#!/usr/bin/env python3
"""Benchmark index refreshes in a disposable tree; never use the real cache."""

import argparse
import json
import os
from pathlib import Path
import queue
import shutil
import statistics
import subprocess
import tempfile
import threading
import time


def frames(stdout, output):
    def records():
        pending = b""
        while chunk := stdout.read1(64 * 1024):
            parts = (pending + chunk).split(b"\0")
            pending = parts.pop()
            yield from parts
        raise EOFError("Index stream closed")

    try:
        records = iter(records())
        record = lambda: next(records)
        while True:
            header = record()
            count = 0
            if header in (b"cached", b"fresh"):
                while record():
                    count += 1
            elif header == b"warning":
                raise RuntimeError(record().decode(errors="replace"))
            output.put((header, count, time.perf_counter()))
    except Exception as error:
        output.put(error)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--files", type=int, default=100_000)
    parser.add_argument("--directories", type=int, default=1_000)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--idle-seconds", type=float, default=35)
    args = parser.parse_args()
    if min(args.files, args.directories, args.runs) < 1 or args.idle_seconds < 0:
        parser.error("Counts must be positive and idle time must be nonnegative")
    binary = str(args.binary.resolve(strict=True))
    scanner = shutil.which("fd")
    shell = shutil.which("sh")
    if not scanner or not shell:
        parser.error("fd and sh must be on PATH")

    with tempfile.TemporaryDirectory(prefix="file-picker-benchmark-") as temporary:
        temporary = Path(temporary)
        root = temporary / "files"
        root.mkdir()
        directories = []
        for number in range(args.directories):
            directory = root / f"group-{number // 100:04d}" / f"dir-{number:06d}"
            directory.mkdir(parents=True)
            directories.append(directory)
        for number in range(args.files):
            (directories[number % len(directories)] / f"file-{number:08d}.txt").touch()

        scans = temporary / "scans"
        scans.touch()
        shim = temporary / "bin"
        shim.mkdir()
        (shim / "fd").write_text(
            f"#!{shell}\n"
            'printf "scan\\n" >> "$FILE_PICKER_BENCH_SCANS"\n'
            'exec "$FILE_PICKER_BENCH_FD" "$@"\n'
        )
        (shim / "fd").chmod(0o700)
        environment = {
            **os.environ,
            "PATH": str(shim) + os.pathsep + os.environ["PATH"],
            "FILE_PICKER_EXECUTABLE": binary,
            "FILE_PICKER_INDEX": str(temporary / "cache/index.sqlite3"),
            "FILE_PICKER_BENCH_SCANS": str(scans),
            "FILE_PICKER_BENCH_FD": scanner,
        }
        command = [binary, "--root", str(root)]

        def run(action):
            started = time.perf_counter()
            result = subprocess.run(
                [*command, action], env=environment, capture_output=True, check=True,
                timeout=120,
            )
            return (time.perf_counter() - started) * 1_000, result.stdout

        def scan_count():
            return len(scans.read_bytes().splitlines())

        def start_feed():
            process = subprocess.Popen(
                [*command, "--index-stream"], env=environment,
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            )
            output = queue.Queue()
            reader = threading.Thread(target=frames, args=(process.stdout, output))
            reader.start()
            return process, reader, output

        def until_ready(output):
            cached_at = None
            while True:
                frame = output.get(timeout=120)
                if isinstance(frame, Exception):
                    raise frame
                header, count, at = frame
                if header in (b"cached", b"fresh"):
                    assert count == args.files, (header, count)
                    if header == b"cached":
                        cached_at = at
                if header == b"ready":
                    return cached_at, at

        def stop_feed(process, reader):
            process.terminate()
            process.wait(timeout=5)
            reader.join(timeout=5)
            process.stdout.close()

        cold_ms, report = run("--index-update")
        assert json.loads(report)["files"] == args.files
        unchanged = []
        cached_reads = []
        for _ in range(args.runs):
            elapsed, report = run("--index-update")
            assert json.loads(report)["added"] == 0
            unchanged.append(elapsed)
            elapsed, paths = run("--cached-list")
            assert paths.count(b"\0") == args.files
            cached_reads.append(elapsed)

        before = scan_count()
        opening = []
        cached_snapshots = []
        for _ in range(args.runs):
            started = time.perf_counter()
            process, reader, output = start_feed()
            try:
                cached_at, ready_at = until_ready(output)
                assert cached_at is not None
                cached_snapshots.append((cached_at - started) * 1_000)
                opening.append((ready_at - started) * 1_000)
            finally:
                stop_feed(process, reader)
        opening_scans = scan_count() - before

        process, reader, output = start_feed()
        try:
            until_ready(output)
            before = scan_count()
            # Read only this child's accounting, with no system-wide tracing.
            def cpu_ticks():
                stat = Path(f"/proc/{process.pid}/stat").read_text().rsplit(")", 1)[1].split()
                return int(stat[11]) + int(stat[12])

            ticks = cpu_ticks()
            time.sleep(args.idle_seconds)
            idle_cpu_ms = (cpu_ticks() - ticks) * 1_000 / os.sysconf("SC_CLK_TCK")
            idle_scans = scan_count() - before
        finally:
            stop_feed(process, reader)

        print(json.dumps({
            "files": args.files,
            "leaf_directories": args.directories,
            "runs": args.runs,
            "cold_refresh_ms": round(cold_ms, 1),
            "unchanged_refresh_median_ms": round(statistics.median(unchanged), 1),
            "cached_list_median_ms": round(statistics.median(cached_reads), 1),
            "cached_snapshot_median_ms": round(statistics.median(cached_snapshots), 1),
            "open_to_ready_median_ms": round(statistics.median(opening), 1),
            "scans_during_repeated_opens": opening_scans,
            "idle_seconds": args.idle_seconds,
            "scans_while_idle": idle_scans,
            "stream_cpu_while_idle_ms": round(idle_cpu_ms, 1),
        }, indent=2))


if __name__ == "__main__":
    main()
