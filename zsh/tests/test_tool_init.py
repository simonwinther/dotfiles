import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SOURCE = Path(__file__).resolve().parents[1] / '.config/zsh/tool-init.zsh'


def make_startup_home(test):
    temp = tempfile.TemporaryDirectory(prefix='zsh-startup-')
    test.addCleanup(temp.cleanup)
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
    return root, env


def run_interactive(root, env, code):
    return subprocess.run(['zsh', '-dic', code], cwd=root, env=env,
                          capture_output=True, text=True, timeout=15)


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
        root, env = make_startup_home(self)
        ids = []
        for _ in range(2):
            result = run_interactive(root, env,
                                     'print -r -- "$POSH_SESSION_ID"; '
                                     'print -r -- "$__dotfiles_zle_vi_mode_label"; '
                                     '_omp_precmd; _omp_get_prompt primary --plain; print; '
                                     '_omp_get_prompt right --plain')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, '')
            lines = result.stdout.splitlines()
            self.assertEqual(lines[1:], ['[I]', ' ~ ❯ '])
            self.assertRegex(lines[0], r'^[0-9a-f-]{36}$')
            ids.append(lines[0])
        self.assertNotEqual(ids[0], ids[1])

    def test_warm_shells_start_without_running_oh_my_posh(self):
        root, env = make_startup_home(self)
        log = root / 'oh-my-posh-calls'
        shim = root / 'bin/oh-my-posh'
        shim.parent.mkdir()
        shim.write_text(f'#!/bin/sh\necho "$*" >> {str(log)!r}\n'
                        f'exec {shutil.which("oh-my-posh")!r} "$@"\n')
        shim.chmod(0o755)
        env['PATH'] = f'{shim.parent}:{env.get("PATH", os.defpath)}'
        for expected_calls in (2, 2):  # generate the cache, then only source it
            result = run_interactive(root, env, 'print -r -- "$_omp_secondary_prompt"')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, '')
            self.assertIn('>', result.stdout)
            self.assertEqual(len(log.read_text().splitlines()), expected_calls)

    def test_prompt_hooks_time_commands_without_starting_oh_my_posh(self):
        root, env = make_startup_home(self)
        result = run_interactive(root, env, '''
            for hook in _omp_preexec _omp_precmd; do
                [[ $functions[$hook] == *'get millis'* ]] && print -r -- "$hook"
            done
            _omp_preexec 'sleep 0.2'; sleep 0.2; _omp_precmd
            print -r -- "$_omp_execution_time"
        ''')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        *hooks_starting_oh_my_posh, elapsed = result.stdout.splitlines()
        self.assertEqual(hooks_starting_oh_my_posh, [])
        self.assertRegex(elapsed, r'^[0-9]+$')
        self.assertGreaterEqual(int(elapsed), 199)
        self.assertLess(int(elapsed), 5000)


class CoreStartupTests(unittest.TestCase):
    def test_completion_keys_bind_in_the_vi_keymap(self):
        root, env = make_startup_home(self)
        # Without 'vi' in either variable, zsh starts in the emacs keymap.
        env.pop('EDITOR', None)
        env.pop('VISUAL', None)
        for _ in range(2):  # full compinit, then the cached dump
            result = run_interactive(root, env, "bindkey -M viins '^Xh'; bindkey -M emacs '^Xh'")
            self.assertEqual(result.returncode, 0, result.stderr)
            vi_binding, emacs_binding = result.stdout.splitlines()
            self.assertEqual(vi_binding, '"^Xh" _complete_help')
            self.assertNotIn('_complete_help', emacs_binding)

    def test_reloads_keep_a_single_atuin_suggestion_strategy(self):
        root, env = make_startup_home(self)
        atuin = root / 'bin/atuin'
        atuin.parent.mkdir()
        # Like Atuin's init, prepend its strategy to any existing ones.
        atuin.write_text('#!/bin/sh\n'
                         "echo 'ZSH_AUTOSUGGEST_STRATEGY=(atuin $ZSH_AUTOSUGGEST_STRATEGY)'\n")
        atuin.chmod(0o755)
        env['PATH'] = f'{atuin.parent}:{env.get("PATH", os.defpath)}'
        # The reload widget sources the configuration inside a function.
        result = run_interactive(root, env, 'reload() { source ~/.zshrc; }; repeat 3 reload; '
                                            'print -r -- "${ZSH_AUTOSUGGEST_STRATEGY[@]}"')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')
        self.assertEqual(result.stdout.splitlines(), ['atuin'])


if __name__ == '__main__':
    unittest.main()
