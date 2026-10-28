from pathlib import Path
import subprocess
import unittest


SOURCE = Path(__file__).resolve().parents[1] / '.config/zsh/vi-mode.zsh'


class ViModeReloadTests(unittest.TestCase):
    def resize_after_sourcing(self, count, previous_handler=''):
        code = previous_handler + '''
            for (( i = 0; i < $2; ++i )); do
                source "$1"
            done
            # Simulate an active editor so the resize must also redraw once.
            zle() { [[ $# == 0 ]] || print -r -- "zle:$*"; }
            FUNCNEST=32
            TRAPWINCH 28
            print -r -- resized
        '''
        return subprocess.run(['zsh', '-dfc', code, 'test', str(SOURCE), str(count)],
                              capture_output=True, text=True, timeout=5)

    def test_reload_without_a_previous_resize_handler(self):
        for count in (1, 2, 3):
            with self.subTest(source_count=count):
                result = self.resize_after_sourcing(count)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, '')
                self.assertEqual(result.stdout.splitlines(), ['zle:reset-prompt', 'resized'])

    def test_reload_preserves_the_previous_resize_handler_once(self):
        for count in (1, 2, 3):
            with self.subTest(source_count=count):
                result = self.resize_after_sourcing(count,
                                                   'TRAPWINCH() { print -r -- previous; }\n')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, '')
                self.assertEqual(result.stdout.splitlines(),
                                 ['previous', 'zle:reset-prompt', 'resized'])


if __name__ == '__main__':
    unittest.main()
