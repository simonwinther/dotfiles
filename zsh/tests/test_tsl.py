import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / '.config/zsh/functions/tsl'


class TslTests(unittest.TestCase):
    def invoke(self, count, fail_target=''):
        with tempfile.TemporaryDirectory(prefix='tsl test ') as directory:
            log = Path(directory) / 'tmux-calls'
            env = dict(os.environ, TMUX='mock-session', TMUX_PANE='%1',
                       TSL_CALL_LOG=str(log), TSL_FAIL_TARGET=fail_target)
            code = '''
                source "$1"
                tmux() {
                    printf '%s\\t' "$@" >> "$TSL_CALL_LOG"
                    printf '\\n' >> "$TSL_CALL_LOG"
                    if [[ $1 == split-window ]]; then
                        if [[ $4 == "$TSL_FAIL_TARGET" ]]; then
                            print -ru2 -- 'mock split failed'
                            return 7
                        fi
                        local target_number="${4#%}"
                        print -r -- "%$(( target_number + 1 ))"
                    fi
                }
                tsl "$2" 'echo hello world'
            '''
            result = subprocess.run(['zsh', '-dfc', code, 'test', str(SOURCE), str(count)],
                                    cwd=directory, env=env, capture_output=True, text=True,
                                    timeout=5)
            calls = [line.split('\t')[:-1] for line in log.read_text().splitlines()]
            return result, calls, directory

    def split_call(self, target, directory):
        return ['split-window', '-h', '-t', target, '-c', directory,
                '-P', '-F', '#{pane_id}']

    def test_selects_the_original_pane(self):
        for count in (1, 3):
            with self.subTest(count=count):
                result, calls, directory = self.invoke(count)
                expected = [['rename-window', '-t', '%1', Path(directory).name]]
                for number in range(1, count):
                    expected.extend([self.split_call(f'%{number}', directory),
                                     ['select-layout', '-t', '%1', 'tiled']])
                expected.extend(['send-keys', '-t', f'%{number}', 'echo hello world', 'C-m']
                                for number in range(1, count + 1))
                expected.append(['select-pane', '-t', '%1'])
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, '')
                self.assertEqual(calls, expected)

    def test_split_failure_stops_immediately(self):
        for failed_split in (1, 2):
            with self.subTest(failed_split=failed_split):
                result, calls, directory = self.invoke(3, fail_target=f'%{failed_split}')
                expected = [['rename-window', '-t', '%1', Path(directory).name]]
                for number in range(1, failed_split):
                    expected.extend([self.split_call(f'%{number}', directory),
                                     ['select-layout', '-t', '%1', 'tiled']])
                expected.append(self.split_call(f'%{failed_split}', directory))
                self.assertEqual(result.returncode, 7, result.stderr)
                self.assertEqual(result.stderr, 'mock split failed\n')
                self.assertEqual(calls, expected)


if __name__ == '__main__':
    unittest.main()
