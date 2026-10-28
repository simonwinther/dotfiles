import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / '.config/zsh/lazy-tools.zsh'


class LazyToolsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zsh-lazy-tools-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.env = dict(os.environ, HOME=str(self.root), PATH=f'{self.bin}:{os.environ.get("PATH", os.defpath)}',
                        NVM_DIR=str(self.root / 'nvm'), SDKMAN_DIR=str(self.root / 'sdk'))

    def run_zsh(self, code):
        return subprocess.run(['zsh', '-dfc', 'source "$1"\n' + code, 'test', str(SOURCE)],
                              env=self.env, capture_output=True, text=True)

    def install_nvm(self, body=None):
        path = self.root / 'nvm/nvm.sh'
        path.parent.mkdir()
        path.write_text(body or 'print loaded >> "$HOME/loads"\nnvm() { print -rl -- "$@"; }\n')

    def test_absent_managers_do_not_wrap_system_commands(self):
        result = self.run_zsh('print -- ${+functions[nvm]} ${+functions[node]} '
                              '${+functions[npm]} ${+functions[npx]} ${+functions[sdk]}')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), '0 0 0 0 0')

    def test_nvm_is_lazy_loads_once_and_preserves_arguments(self):
        self.install_nvm()
        result = self.run_zsh('[[ ! -e "$HOME/loads" ]] || exit 90\n'
                              'nvm "argument with spaces" \'literal;$()\'\nnvm second')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ['argument with spaces', 'literal;$()', 'second'])
        self.assertEqual((self.root / 'loads').read_text().splitlines(), ['loaded'])

    def test_node_npm_and_npx_initialize_without_calling_nvm_help(self):
        for name in ('node', 'npm', 'npx'):
            with self.subTest(name=name):
                if not (self.root / 'nvm/nvm.sh').exists():
                    self.install_nvm('print loaded >> "$HOME/loads"\n'
                                     'nvm() { print unwanted-help; return 91; }\n')
                command = self.bin / name
                command.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
                command.chmod(0o700)
                result = self.run_zsh(f'{name} "two words" tail')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.splitlines(), ['two words', 'tail'])

    def test_loader_failure_is_propagated(self):
        self.install_nvm('return 23\n')
        result = self.run_zsh('nvm --version')
        self.assertEqual(result.returncode, 23)

    def test_missing_nvm_definition_cannot_recurse(self):
        self.install_nvm('true\n')
        result = self.run_zsh('nvm --version')
        self.assertEqual(result.returncode, 127)
        self.assertIn('did not define', result.stderr)

    def test_sdk_is_lazy_and_calls_the_loaded_function(self):
        path = self.root / 'sdk/bin/sdkman-init.sh'
        path.parent.mkdir(parents=True)
        path.write_text('print loaded >> "$HOME/loads"\nsdk() { print -rl -- "$@"; }\n')
        result = self.run_zsh('[[ ! -e "$HOME/loads" ]] || exit 90\n'
                              'sdk "two words" version\nsdk second')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ['two words', 'version', 'second'])
        self.assertEqual((self.root / 'loads').read_text().splitlines(), ['loaded'])


if __name__ == '__main__':
    unittest.main()
