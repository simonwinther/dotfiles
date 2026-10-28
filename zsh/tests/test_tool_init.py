import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / '.config/zsh/tool-init.zsh'


class ToolInitTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='zsh-tool-init-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.tool = self.root / 'bin/mock-tool'
        self.tool.parent.mkdir()
        self.tool.write_text('#!/bin/sh\necho generated >> "$HOME/generations"\n'
                             'printf "typeset -g tool_loaded=yes\\n"\n')
        self.tool.chmod(0o755)
        self.config = self.root / 'config with spaces'
        self.config.write_text('settings')
        self.env = dict(os.environ, HOME=str(self.root), XDG_CACHE_HOME=str(self.root / 'cache'),
                        PATH=f'{self.tool.parent}:{os.environ.get("PATH", os.defpath)}')

    def invoke(self, args='init zsh'):
        code = 'source "$1"\n__dotfiles_cached_init mock-tool "$2" ' + args + '\nprint -- "$tool_loaded"'
        return subprocess.run(['zsh', '-dfc', code, 'test', str(SOURCE), str(self.config)],
                              env=self.env, capture_output=True, text=True)

    def test_warm_start_sources_without_starting_the_tool(self):
        for _ in range(2):
            result = self.invoke()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), 'yes')
        self.assertEqual((self.root / 'generations').read_text().splitlines(), ['generated'])
        self.assertEqual((self.root / 'cache/zsh/mock-tool-init.zsh').stat().st_mode & 0o777, 0o600)

    def test_config_changes_invalidate_the_cache(self):
        self.assertEqual(self.invoke().returncode, 0)
        now = self.config.stat().st_mtime + 10
        os.utime(self.config, (now, now))
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(len((self.root / 'generations').read_text().splitlines()), 2)

    def test_argument_changes_invalidate_the_cache(self):
        self.assertEqual(self.invoke().returncode, 0)
        self.assertEqual(self.invoke('init zsh --different').returncode, 0)
        self.assertEqual(len((self.root / 'generations').read_text().splitlines()), 2)

    def test_binary_replacement_with_older_timestamp_invalidates_the_cache(self):
        self.assertEqual(self.invoke().returncode, 0)
        self.tool.write_text('#!/bin/sh\nprintf "typeset -g tool_loaded=updated\\n"\n')
        os.utime(self.tool, (1, 1))
        self.assertEqual(self.invoke().stdout.strip(), 'updated')

    def test_failed_generation_keeps_the_previous_cache_intact(self):
        self.assertEqual(self.invoke().returncode, 0)
        cache = self.root / 'cache/zsh/mock-tool-init.zsh'
        original = cache.read_bytes()
        self.tool.write_text('#!/bin/sh\necho partial\nexit 7\n')
        code = 'source "$1"\n__dotfiles_cached_init mock-tool "$2" changed'
        result = subprocess.run(['zsh', '-dfc', code, 'test', str(SOURCE), str(self.config)],
                                env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(cache.read_bytes(), original)
        self.assertFalse(list(cache.parent.glob('*.tmp')))


@unittest.skipUnless(shutil.which('oh-my-posh'), 'oh-my-posh is required')
class PromptStartupTests(unittest.TestCase):
    def test_fresh_and_warm_shells_render_the_personal_prompt(self):
        temp = tempfile.TemporaryDirectory(prefix='zsh-prompt-')
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        package = SOURCE.parents[2]
        shutil.copytree(package / '.config/zsh', root / '.config/zsh')
        shutil.copytree(package.parent / 'oh-my-posh/.config/oh-my-posh',
                        root / '.config/oh-my-posh')
        shutil.copyfile(package / '.zshrc', root / '.zshrc')
        env = dict(os.environ, HOME=str(root), XDG_CACHE_HOME=str(root / '.cache'),
                   XDG_CONFIG_HOME=str(root / '.config'),
                   XDG_DATA_HOME=str(root / '.local/share'),
                   XDG_STATE_HOME=str(root / '.local/state'))
        env.pop('ZDOTDIR', None)
        env.pop('POSH_SESSION_ID', None)
        ids = []
        for _ in range(2):
            result = subprocess.run(['zsh', '-dic',
                                     'print -r -- "$POSH_SESSION_ID"; '
                                     'print -r -- "$__dotfiles_zle_vi_mode_label"; '
                                     'oh-my-posh print primary --shell zsh --plain; print; '
                                     'oh-my-posh print right --shell zsh --plain'],
                                    cwd=root, env=env, capture_output=True, text=True,
                                    timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, '')
            lines = result.stdout.splitlines()
            self.assertEqual(lines[1:], ['[I]', ' ~ ❯ '])
            self.assertRegex(lines[0], r'^[0-9a-f-]{36}$')
            ids.append(lines[0])
        self.assertNotEqual(ids[0], ids[1])


if __name__ == '__main__':
    unittest.main()
