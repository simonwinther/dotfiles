#!/usr/bin/env python3
"""On-demand KU VPN and saved-password login for the Hendrix SSH alias."""

import fcntl
import getpass
import json
import os
from pathlib import Path
import re
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time

BASE = Path(__file__).resolve().parent
CONFIG = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'hendrix'
PASSWORD = CONFIG / 'ku-password'
# The installed VPN service reads this existing path as its standard input.
VPN_PASSWORD = CONFIG / 'password'
VPN_AUTH = CONFIG / 'vpn-auth.json'
STATUS = CONFIG / 'setup-status.json'
SSH = os.environ.get('DESKTOP_SSH_BIN', '/run/current-system/sw/bin/ssh' if Path('/run/current-system/sw/bin/ssh').exists() else '/usr/bin/ssh')
HOSTS = {'hendrix', 'hendrixgate', 'hendrixgate.unicph.domain'}
ARG_OPTIONS = set('BbcDEeFIiJLlmOopQRSWw')


def destination(args):
    """Find the destination without mistaking an option's argument for a host."""
    skip = False
    literal = False
    inspect_only = False
    user = None
    for index, arg in enumerate(args):
        if skip:
            skip = False
            continue
        if not literal and arg == '--':
            literal = True
            continue
        if not literal and arg.startswith('-') and arg != '-':
            for offset, letter in enumerate(arg[1:], 1):
                if letter in 'GVQO':
                    inspect_only = True
                if letter in ARG_OPTIONS:
                    value = arg[offset + 1:]
                    if not value and index + 1 < len(args):
                        value = args[index + 1]
                        skip = True
                    if letter == 'l':
                        user = value
                    break
            continue
        if inspect_only:
            return None
        prefix, _, host = arg.rpartition('@')
        if host not in HOSTS or (prefix and prefix != 'zlp616'):
            return None
        if user and user != 'zlp616':
            return None
        return arg
    return None


def askpass_env(password_file=PASSWORD):
    env = os.environ.copy()
    env.update(SSH_ASKPASS=str(BASE / 'askpass'), SSH_ASKPASS_REQUIRE='force',
               HENDRIX_PASSWORD_FILE=str(password_file))
    return env


def askpass(prompt):
    # Supply this credential only to the expected Hendrix password prompt.
    if 'password' not in prompt.lower() or 'zlp616@hendrixgate' not in prompt.lower():
        return 1
    path = Path(os.environ.get('HENDRIX_PASSWORD_FILE', str(PASSWORD)))
    # Older running wrappers may still pass the original shared password path.
    if path == VPN_PASSWORD:
        path = PASSWORD
    try:
        data = path.read_bytes().rstrip(b'\n')
    except OSError:
        return 1
    if not data:
        return 1
    sys.stdout.buffer.write(data + b'\n')
    return 0


def probe(host='hendrixgate.unicph.domain', port=22):
    try:
        with socket.create_connection((host, port), timeout=2):
            return 0
    except OSError:
        return 1


def reachable():
    # Bound DNS lookup time too, since the short name requires KU DNS.
    try:
        return subprocess.run([sys.executable, str(BASE / 'hendrix.py'), 'probe'],
                              timeout=3, stdin=subprocess.DEVNULL,
                              stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL).returncode == 0
    except subprocess.TimeoutExpired:
        return False


def prepare_vpn_credentials():
    password = PASSWORD.read_bytes().rstrip(b'\n')
    config = json.loads(VPN_AUTH.read_text()) if VPN_AUTH.exists() else {'mode': 'push'}
    mode = config.get('mode', 'push')
    if mode in ('firefox', 'totp'):
        from firefox_totp import generate
        if mode == 'totp':
            from saved_totp import read_entry
        else:
            from firefox_totp import read_entry
        period = 30
        try:
            entry = read_entry()
            period = int(entry.get('period') or 30)
            if not 1 <= period <= 3600:
                raise ValueError('Invalid authenticator period.')
            now = time.time()
            counter = int(now // period)
            remaining = period - now % period
            if counter <= config.get('last_auth_counter', -1) or remaining < 8:
                time.sleep(remaining + 0.2)
            code = generate(entry['secret'], period=period, digits=int(entry.get('digits') or 6))
        except (ValueError, OSError, sqlite3.Error, KeyError):
            period = 30
            if not sys.stdin.isatty():
                raise RuntimeError('The KU authenticator is unavailable. Retry ssh hendrix in a terminal to enter its code.') from None
            code = getpass.getpass('KU code from Firefox Authenticator: ').strip()
            if not re.fullmatch(r'\d{6}', code):
                raise RuntimeError('Enter the six-digit KU authenticator code.') from None
        password += b'&' + code.encode()
        config['last_auth_counter'] = int(time.time() // period)
    descriptor, temporary = tempfile.mkstemp(prefix='.vpn-input-', dir=CONFIG)
    try:
        with os.fdopen(descriptor, 'wb') as out:
            out.write(password + b'\n')
        os.replace(temporary, VPN_PASSWORD)
    finally:
        Path(temporary).unlink(missing_ok=True)
    if mode in ('firefox', 'totp'):
        VPN_AUTH.write_text(json.dumps(config) + '\n')
        VPN_AUTH.chmod(0o600)
    return config.get('mode', 'push')


def ensure_vpn():
    if reachable():
        return
    CONFIG.mkdir(mode=0o700, parents=True, exist_ok=True)
    with (CONFIG / 'connect.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if reachable():
            return
        runner = Path(os.environ.get('HENDRIX_VPN_RUNNER', '/run/current-system/sw/bin/ku-vpn'))
        routes = Path(os.environ.get('HENDRIX_VPN_ROUTES', '/run/current-system/sw/bin/ku-vpn-routes'))
        if not routes.is_file() or 'HENDRIX_SPLIT_TUNNEL_V1' not in runner.read_text():
            raise RuntimeError('The internet-preserving VPN update needs to finish before connecting.')
        mode = prepare_vpn_credentials()
        print('Starting KU VPN with your saved authenticator…' if mode in ('firefox', 'totp') else
              'Starting KU VPN — approve the KU phone notification if requested…',
              file=sys.stderr, flush=True)
        result = subprocess.run(
            ['sudo', '-n', 'systemctl', 'start', 'ku-vpn.service'])
        if result.returncode:
            raise RuntimeError('KU VPN setup is incomplete. Run hendrix-setup once.')
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            if reachable():
                return
            if subprocess.run(['systemctl', 'is-failed', '--quiet',
                               'ku-vpn.service']).returncode == 0:
                raise RuntimeError('KU VPN authentication failed. Retry ssh hendrix for a fresh authenticator code.' if mode in ('firefox', 'totp') else
                                   'KU VPN login failed or phone approval timed out. Retry ssh hendrix and approve the KU notification.')
            time.sleep(1)
        raise RuntimeError('KU VPN did not become reachable within 120 seconds.')


def ssh(args):
    target = destination(args)
    if not target:
        os.execv(SSH, [SSH, *args])
    # Until setup completes, preserve the existing interactive SSH behaviour.
    if not PASSWORD.is_file():
        os.execv(SSH, [SSH, *args])
    ensure_vpn()
    os.execve(SSH, [SSH, '-o', 'PreferredAuthentications=publickey,password,keyboard-interactive',
                   '-o', 'NumberOfPasswordPrompts=1', *args], askpass_env())


def status(step, **extra):
    STATUS.write_text(json.dumps({'step': step, **extra}) + '\n')
    STATUS.chmod(0o600)


def check_ssh(password_file):
    return subprocess.run(
        [SSH, '-o', 'ControlMaster=no', '-o', 'ControlPath=none',
         '-o', 'PreferredAuthentications=password', '-o', 'PubkeyAuthentication=no',
         '-o', 'NumberOfPasswordPrompts=1',
         '-o', 'ConnectTimeout=12', 'hendrix', 'true'],
        env=askpass_env(password_file), stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=20).returncode == 0


def setup():
    CONFIG.mkdir(mode=0o700, parents=True, exist_ok=True)
    CONFIG.chmod(0o700)
    status('waiting-for-ku-password')
    print('Set up automatic Hendrix login and KU VPN.\n')
    print('Enter your KU password here, then your Linux password for sudo.')
    print('The KU password is saved locally and used automatically afterward.\n')
    while True:
        password = getpass.getpass('KU password (Hendrix and VPN): ').encode()
        if not password:
            continue
        descriptor, pending_name = tempfile.mkstemp(prefix='.password-', dir=CONFIG)
        pending = Path(pending_name)
        try:
            with os.fdopen(descriptor, 'wb') as out:
                out.write(password + b'\n')
            status('validating-hendrix')
            print('Checking a fresh Hendrix login…', flush=True)
            if reachable() and not check_ssh(pending):
                print('Hendrix rejected that password. Please try again.\n')
                status('waiting-for-ku-password')
                continue
            os.replace(pending, PASSWORD)
            break
        finally:
            pending.unlink(missing_ok=True)
    del password
    finish_setup()


def internet_works():
    for url in ('https://www.google.com/generate_204', 'https://github.com'):
        result = subprocess.run(['curl', '--silent', '--show-error',
                                 '--output', '/dev/null', '--connect-timeout', '3',
                                 '--max-time', '5', url],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if result.returncode:
            return False
    return True


def finish_setup():
    status('waiting-for-sudo')
    print('\nInstalling the background VPN service. Enter your Linux sudo password.', flush=True)
    mode = prepare_vpn_credentials()
    if mode in ('firefox', 'totp'):
        print('KU authentication uses your saved authenticator automatically.', flush=True)
    else:
        print('Then approve the KU notification on your phone when the VPN starts.', flush=True)
    result = subprocess.run(['systemctl', 'cat', 'ku-vpn.service'], stdout=subprocess.DEVNULL)
    if result.returncode:
        raise RuntimeError('Service installation did not finish. Run hendrix-setup to retry.')
    status('checking-background-vpn')
    ensure_vpn()
    if not internet_works():
        raise RuntimeError('The normal internet checks failed. The VPN update needs inspection.')
    print('Checking fresh SSH login through the background VPN…', flush=True)
    if not check_ssh(PASSWORD):
        raise RuntimeError('VPN connected, but fresh Hendrix SSH authentication failed.')
    status('complete', ssh_verified=True, vpn_verified=True, internet_verified=True)
    print('\nDone. Normal internet and fresh Hendrix login both work.')
    print('In a new terminal, ssh hendrix now connects automatically.')
    input('\nPress Enter to close this window. ')


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ''
    try:
        if mode == 'askpass':
            return askpass(' '.join(sys.argv[2:]))
        if mode == 'probe':
            return probe()
        if mode == 'ssh':
            ssh(sys.argv[2:])
        elif mode == 'setup':
            setup()
        elif mode == 'repair':
            print('Install the KU VPN correction: only Hendrix traffic goes through KU.')
            print('Your saved KU password is reused. Enter your Linux sudo password once.\n')
            finish_setup()
        else:
            return 2
        return 0
    except (RuntimeError, OSError, subprocess.TimeoutExpired) as error:
        message = str(error) if not isinstance(error, subprocess.TimeoutExpired) else 'Connection timed out. Run hendrix-setup to retry.'
        print(message, file=sys.stderr)
        if mode in ('setup', 'repair'):
            status('error', message=message)
            input('\nPress Enter to close this window. ')
        return 1
    except (KeyboardInterrupt, EOFError):
        if mode in ('setup', 'repair'):
            status('cancelled')
        return 130


if __name__ == '__main__':
    sys.exit(main())
