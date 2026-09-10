#!/usr/bin/env python3
"""Small, cached wallpaper grid; originals and Omarchy source are read-only.

The shared awww setter maintains Omarchy's selected-background symlink.
GTK work stays on the main thread; decoding and wallpaper application do not.
"""
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
import configparser
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
HOME_DIR = Path.home()
CACHE = HOME_DIR / '.cache/background-grid'
EXTENSIONS = {'.jpg', '.jpeg', '.png', '.webp', '.bmp', '.gif', '.tif', '.tiff', '.avif'}
COLUMNS = 4
WINDOW_WIDTH = 900


def image_folders():
    # An explicit personal library takes precedence over Omarchy's theme copies.
    # Stow keeps ROOT inside this checkout, so relative paths work on either PC.
    library = ROOT / 'folders.json'
    if library.exists():
        configured = [(ROOT / p).resolve() for p in json.loads(library.read_text())]
        if any(p.is_dir() for p in configured):
            return configured
    current = HOME_DIR / '.config/omarchy/current'
    folders = [current / 'theme/backgrounds']
    name = current / 'theme.name'
    if name.exists():
        folders.append(HOME_DIR / '.config/omarchy/backgrounds' / name.read_text().strip())
    config = configparser.ConfigParser(interpolation=None)
    config.read(HOME_DIR / '.config/waypaper/config.ini')
    folders.extend(Path(p).expanduser() for p in config.get('Settings', 'folder', fallback='').splitlines() if p)
    return folders


@dataclass
class Folder:
    name: str
    images: list[Path] = field(default_factory=list)
    children: dict = field(default_factory=dict)

    def all_images(self):
        return self.images + [path for child in self.children.values() for path in child.all_images()]


def catalog(folders=None):
    """Keep folder placement; deduplicate alias roots and stop symlink cycles.

    Reading the catalog never creates folders or moves an image. Matching category
    names from different configured roots are merged, while nested folders remain nested.
    """
    root, seen_dirs = Folder(''), set()

    def scan(directory, node):
        real = directory.resolve()
        if real in seen_dirs or not directory.is_dir():
            return
        seen_dirs.add(real)
        for path in sorted(directory.iterdir(), key=lambda p: p.name.casefold()):
            if path.name.startswith('.'):
                continue
            if path.is_dir() and path.resolve() not in seen_dirs:
                child = node.children.setdefault(path.name, Folder(path.name))
                scan(path, child)
            elif path.is_file() and path.suffix.lower() in EXTENSIONS:
                resolved = path.resolve()
                if resolved not in node.images:
                    node.images.append(resolved)
        node.images.sort(key=lambda p: (p.name.casefold(), str(p)))

    for folder in image_folders() if folders is None else folders:
        scan(Path(folder), root)
    return root


def images():
    return sorted(set(catalog().all_images()), key=lambda p: (p.name.casefold(), str(p)))


def thumbnail(path):
    from PIL import Image, ImageOps
    stat = path.stat()
    key = hashlib.sha256(f'v1:{path}:{stat.st_mtime_ns}:{stat.st_size}'.encode()).hexdigest()
    target = CACHE / f'{key}.png'
    if not target.exists():
        CACHE.mkdir(parents=True, exist_ok=True)
        with Image.open(path) as source:
            source.draft('RGB', (896, 504))
            small = ImageOps.fit(ImageOps.exif_transpose(source).convert('RGB'),
                                (448, 252), method=Image.Resampling.LANCZOS)
            temporary = target.with_suffix(f'.{os.getpid()}.tmp')
            small.save(temporary, format='PNG', optimize=False)
            temporary.replace(target)
    return target


def wallpaper_ready():
    """Check the renderer's actual layer and owning process."""
    pids = subprocess.run(['pgrep', '-u', str(os.getuid()), '-x', 'awww-daemon'],
                          capture_output=True, text=True).stdout.split()
    layers = json.loads(subprocess.check_output(['hyprctl', '-j', 'layers'], text=True))
    return any(str(layer.get('pid')) in pids and layer.get('namespace') == 'awww-daemon'
               for monitor in layers.values() for level in monitor.get('levels', {}).values()
               for layer in level)


def set_wallpaper(path, transition=None):
    path = path.expanduser().resolve()
    if not path.is_file():
        raise RuntimeError('This image is no longer available.')
    animation = json.loads((ROOT / 'transition.json').read_text())
    current = HOME_DIR / '.config/omarchy/current/background'
    if current.exists() and not current.is_symlink():
        raise RuntimeError('The current background must be a symlink; refusing to replace an image.')
    CACHE.mkdir(parents=True, exist_ok=True)
    with (CACHE / 'apply.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        running = subprocess.run(['awww', 'query'], capture_output=True).returncode == 0
        if not running:
            subprocess.Popen(['uwsm-app', '--', 'awww-daemon', '--no-cache'],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
            for _ in range(80):
                if subprocess.run(['awww', 'query'], capture_output=True).returncode == 0:
                    break
                time.sleep(.05)
            else:
                raise RuntimeError('The wallpaper renderer did not start.')
            # Bootstrap from the currently visible image before the first fade.
            previous = current.resolve()
            if previous.is_file():
                subprocess.run(['awww', 'img', '--transition-type', 'none', str(previous)],
                               capture_output=True, check=True, timeout=15)
        subprocess.run(['awww', 'img', '--resize', 'crop', '--transition-type', transition or animation['type'],
                        '--transition-duration', str(animation['duration']),
                        '--transition-fps', str(animation['fps']),
                        '--transition-step', str(animation['step']), str(path)],
                       capture_output=True, check=True, timeout=15)
        for _ in range(60):
            if wallpaper_ready():
                break
            time.sleep(.05)
        else:
            raise RuntimeError('The wallpaper renderer has no visible surface.')
        temporary = current.with_name('background-grid.next')
        if temporary.is_symlink():
            temporary.unlink()
        temporary.symlink_to(path)
        temporary.replace(current)
        # Omarchy may start swaybg on login; stop it only after our image is ready.
        subprocess.run(['pkill', '-u', str(os.getuid()), '-x', 'swaybg'], capture_output=True)


def launch(folders=None):
    import gi
    gi.require_version('Gtk', '4.0')
    from gi.repository import Gtk, Gdk, Gio, GLib

    class Grid(Gtk.Application):
        def __init__(self):
            super().__init__(application_id='com.simon.BackgroundGrid', flags=Gio.ApplicationFlags.DEFAULT_FLAGS)
            self.pool = ThreadPoolExecutor(max_workers=2)
            self.window = None
            self.closed = False
            self.busy = False
            self.connect('activate', self.activate)
            self.connect('shutdown', self.shutdown)

        def shutdown(self, *_):
            self.closed = True
            self.pool.shutdown(wait=False, cancel_futures=True)

        def activate(self, *_):
            if self.window:
                self.window.present()
                return
            self.started = time.monotonic()
            self.window = Gtk.ApplicationWindow(application=self, title='Backgrounds')
            self.window.set_decorated(False)
            self.window.set_resizable(False)
            # File enumeration is cheap; decoding remains in the worker pool.
            # Size to show the current collection, with scrolling for larger libraries.
            collection = catalog(folders)
            self.window.set_default_size(WINDOW_WIDTH, -1)
            self.window.add_css_class('background-grid')
            css = Gtk.CssProvider()
            css.load_from_path(str(ROOT / 'style.css'))
            Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), css, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
            outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=14)
            for edge in ('top', 'bottom', 'start', 'end'):
                getattr(outer, f'set_margin_{edge}')(20)
            self.window.set_child(outer)
            header = Gtk.Box(spacing=12)
            title = Gtk.Label(label='Backgrounds', xalign=0)
            title.add_css_class('heading')
            header.append(title)
            self.count = Gtk.Label(xalign=0)
            self.count.add_css_class('muted')
            self.count.set_hexpand(True)
            header.append(self.count)
            close = Gtk.Button.new_from_icon_name('window-close-symbolic')
            close.add_css_class('close-button')
            close.set_tooltip_text('Close (q / Esc)')
            close.connect('clicked', lambda *_: self.quit())
            header.append(close)
            outer.append(header)
            self.sections = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
            self.scroll = Gtk.ScrolledWindow()
            self.scroll.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
            self.scroll.set_propagate_natural_height(True)
            self.scroll.set_max_content_height(550)
            self.scroll.set_child(self.sections)
            outer.append(self.scroll)
            self.hint = 'H J K L / arrows  ·  Enter opens / applies  ·  Ctrl+click / Enter keeps open  ·  q / Esc closes'
            self.status = Gtk.Label(label=self.hint, xalign=0)
            self.status.add_css_class('muted')
            outer.append(self.status)
            keys = Gtk.EventControllerKey()
            keys.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
            keys.connect('key-pressed', self.key)
            self.window.add_controller(keys)
            self.focused = None
            self.category_headers = {}
            self.groups = []
            self.root_buttons = []
            self.image_buttons = {}
            self.activation_keep_open = False
            self.remaining = 0
            self.loaded_images = 0
            self.active = (HOME_DIR / '.config/omarchy/current/background').resolve()
            self.populate(collection)
            self.window.present()

        def key(self, _, key, _keycode, state):
            if key in (Gdk.KEY_q, Gdk.KEY_Escape):
                self.quit()
                return True
            rows = self.visible_rows()
            controls = [button for row in rows for button in row]
            if not controls or self.busy:
                return False
            movement = {Gdk.KEY_Left: 'left', Gdk.KEY_h: 'left', Gdk.KEY_Right: 'right', Gdk.KEY_l: 'right',
                        Gdk.KEY_Up: 'up', Gdk.KEY_k: 'up', Gdk.KEY_Down: 'down', Gdk.KEY_j: 'down'}
            if key in movement or key in (Gdk.KEY_Return, Gdk.KEY_KP_Enter, Gdk.KEY_space):
                focus = self.window.get_focus()
                while focus and focus not in controls:
                    focus = focus.get_parent()
                focus = focus or (self.focused if self.focused in controls else controls[0])
                if key in movement:
                    direction = movement[key]
                    group = self.category_headers.get(focus)
                    if group and direction in ('left', 'right'):
                        if direction == 'left' and group['expanded']:
                            self.toggle_group(focus, group)
                        elif direction == 'right' and not group['expanded']:
                            self.toggle_group(focus, group)
                        elif direction == 'right' and group['expanded']:
                            flat = [b for row in self.visible_rows() for b in row]
                            self.focus_tile(flat[min(len(flat)-1, flat.index(focus)+1)])
                    elif direction in ('up', 'down'):
                        row_index = next(i for i, row in enumerate(rows) if focus in row)
                        column = rows[row_index].index(focus)
                        target = rows[max(0, min(len(rows)-1, row_index + (1 if direction == 'down' else -1)))]
                        self.focus_tile(target[min(column, len(target)-1)])
                    else:
                        index = controls.index(focus) + (1 if direction == 'right' else -1)
                        self.focus_tile(controls[max(0, min(len(controls)-1, index))])
                else:
                    self.activation_keep_open = bool(state & Gdk.ModifierType.CONTROL_MASK)
                    focus.emit('clicked')
                return True
            return False

        def visible_rows(self):
            def image_rows(buttons):
                return [buttons[i:i+COLUMNS] for i in range(0, len(buttons), COLUMNS)]

            def group_rows(group):
                result = [[group['header']]]
                if group['expanded']:
                    result.extend(image_rows(group['buttons']))
                    for child in group['children']:
                        result.extend(group_rows(child))
                return result

            result = image_rows(self.root_buttons)
            for group in self.groups:
                result.extend(group_rows(group))
            return result

        def focus_tile(self, button):
            if self.focused:
                self.focused.remove_css_class('keyboard-focus')
            self.focused = button
            button.add_css_class('keyboard-focus')
            button.grab_focus()
            GLib.idle_add(self.ensure_visible, button)
            if os.environ.get('BACKGROUND_GRID_DEBUG'):
                controls = [b for row in self.visible_rows() for b in row]
                print(f'Focus: {controls.index(button)}', flush=True)

        def ensure_visible(self, button):
            if self.closed:
                return False
            success, bounds = button.compute_bounds(self.sections)
            if success:
                adjustment = self.scroll.get_vadjustment()
                top, bottom = bounds.get_y(), bounds.get_y() + bounds.get_height()
                value, page = adjustment.get_value(), adjustment.get_page_size()
                if top < value:
                    adjustment.set_value(max(0, top-4))
                elif bottom > value + page:
                    adjustment.set_value(bottom-page+4)
            return False

        def resize_to_content(self):
            # Reset the previous height so GTK fits the visible content, up to
            # the scroller's height limit, even after collapsing a tall category.
            self.window.set_default_size(WINDOW_WIDTH, -1)

        def reveal_group(self, header):
            if not self.closed:
                success, bounds = header.compute_bounds(self.sections)
                if success:
                    adjustment = self.scroll.get_vadjustment()
                    adjustment.set_value(min(bounds.get_y(), max(0, adjustment.get_upper()-adjustment.get_page_size())))
            return False

        def populate(self, collection):
            if self.closed:
                return False
            self.count.set_label(f'{len(set(collection.all_images()))} images')
            self.root_buttons = self.add_grid(self.sections, collection.images)
            for node in sorted(collection.children.values(), key=lambda n: n.name.casefold()):
                self.groups.append(self.add_group(self.sections, node))
            controls = [b for row in self.visible_rows() for b in row]
            if controls:
                selected = next((b for b in self.root_buttons if b.has_css_class('current')), controls[0])
                self.focus_tile(selected)
            else:
                self.status.set_label('No images found in your wallpaper folders.')
            self.resize_to_content()
            return False

        def add_group(self, parent, node):
            container = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
            parent.append(container)
            header = Gtk.Button()
            header.add_css_class('category')
            line = Gtk.Box(spacing=10)
            arrow = Gtk.Image.new_from_icon_name('pan-end-symbolic')
            line.append(arrow)
            label = Gtk.Label(label=node.name, xalign=0)
            label.set_hexpand(True)
            line.append(label)
            count = Gtk.Label(label=str(len(set(node.all_images()))))
            count.add_css_class('muted')
            line.append(count)
            header.set_child(line)
            header.update_property([Gtk.AccessibleProperty.LABEL], [f'{node.name}, {count.get_label()} images'])
            header.update_state([Gtk.AccessibleState.EXPANDED], [0])
            container.append(header)
            body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
            body.set_visible(False)
            container.append(body)
            group = dict(node=node, header=header, body=body, arrow=arrow,
                         expanded=False, loaded=False, buttons=[], children=[])
            self.category_headers[header] = group
            header.connect('clicked', self.toggle_group, group)
            return group

        def toggle_group(self, header, group):
            group['expanded'] = not group['expanded']
            if group['expanded'] and not group['loaded']:
                group['loaded'] = True
                node = group['node']
                group['buttons'] = self.add_grid(group['body'], node.images)
                for child in sorted(node.children.values(), key=lambda n: n.name.casefold()):
                    group['children'].append(self.add_group(group['body'], child))
                if not node.images and not node.children:
                    empty = Gtk.Label(label='No images in this folder yet', xalign=0)
                    empty.add_css_class('muted')
                    group['body'].append(empty)
            group['body'].set_visible(group['expanded'])
            group['arrow'].set_from_icon_name('pan-down-symbolic' if group['expanded'] else 'pan-end-symbolic')
            header.update_state([Gtk.AccessibleState.EXPANDED], [int(group['expanded'])])
            self.resize_to_content()
            self.focus_tile(header)
            if group['expanded']:
                GLib.timeout_add(80, self.reveal_group, header)
            if os.environ.get('BACKGROUND_GRID_DEBUG'):
                print(f"Category: {group['node'].name} expanded={group['expanded']}", flush=True)

        def add_grid(self, parent, paths):
            if not paths:
                return []
            flow = Gtk.FlowBox()
            flow.set_homogeneous(True)
            flow.set_min_children_per_line(COLUMNS)
            flow.set_max_children_per_line(COLUMNS)
            flow.set_row_spacing(12)
            flow.set_column_spacing(12)
            flow.set_selection_mode(Gtk.SelectionMode.NONE)
            flow.set_valign(Gtk.Align.START)
            parent.append(flow)
            buttons = []
            self.remaining += len(paths)
            for path in paths:
                button = Gtk.Button()
                button.add_css_class('tile')
                button.update_property([Gtk.AccessibleProperty.LABEL], [f'Wallpaper: {path.stem}'])
                picture = Gtk.Picture()
                picture.set_content_fit(Gtk.ContentFit.COVER)
                picture.set_can_shrink(True)
                picture.set_size_request(196, 126)
                button.set_child(picture)
                if path == self.active:
                    button.add_css_class('current')
                button.set_tooltip_text('Ctrl-click to apply and keep the picker open')
                click = Gtk.GestureClick()
                click.set_button(1)
                click.set_propagation_phase(Gtk.PropagationPhase.CAPTURE)
                click.connect('pressed', self.pointer_modifiers)
                button.add_controller(click)
                button.connect('clicked', self.choose, path)
                self.image_buttons[button] = path
                buttons.append(button)
                flow.append(button)
                self.pool.submit(self.decode, path, picture, button)
            return buttons

        def decode(self, path, picture, button):
            try:
                cached = thumbnail(path)
                GLib.idle_add(self.show_image, picture, button, cached, None)
            except Exception as error:
                GLib.idle_add(self.show_image, picture, button, None, str(error))

        def show_image(self, picture, button, cached, error):
            if self.closed:
                return False
            if cached:
                picture.set_filename(str(cached))
            else:
                button.set_child(Gtk.Label(label='Preview unavailable'))
                button.set_tooltip_text(error)
            self.remaining -= 1
            self.loaded_images += 1
            if self.remaining == 0:
                print(f'Grid ready: {self.loaded_images} previews in {time.monotonic()-self.started:.3f}s', flush=True)
            return False

        def pointer_modifiers(self, gesture, *_):
            self.activation_keep_open = bool(gesture.get_current_event_state() & Gdk.ModifierType.CONTROL_MASK)

        def choose(self, button, path):
            keep_open = self.activation_keep_open
            self.activation_keep_open = False
            if self.busy:
                return
            self.focus_tile(button)
            self.busy = True
            self.status.set_label('Applying…')
            self.sections.set_sensitive(False)
            if not keep_open:
                self.window.set_visible(False)
            self.pool.submit(self.apply, path, keep_open)

        def apply(self, path, keep_open):
            try:
                set_wallpaper(path)
                GLib.idle_add(self.applied, path, keep_open)
            except Exception as error:
                GLib.idle_add(self.error, str(error))

        def applied(self, path, keep_open):
            if self.closed:
                return False
            if keep_open:
                self.active = path
                for button, candidate in self.image_buttons.items():
                    if candidate == path:
                        button.add_css_class('current')
                    else:
                        button.remove_css_class('current')
                self.sections.set_sensitive(True)
                self.busy = False
                self.status.set_label(self.hint)
                if os.environ.get('BACKGROUND_GRID_DEBUG'):
                    print('Applied: keep-open', flush=True)
            else:
                self.quit()
            return False

        def error(self, message):
            if not self.closed:
                self.status.set_label(message)
                self.sections.set_sensitive(True)
                self.busy = False
                self.window.present()
            return False

    return Grid().run(sys.argv)


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--set':
        set_wallpaper(Path(sys.argv[2]))
    elif sys.argv[1:] == ['--restore']:
        set_wallpaper((HOME_DIR / '.config/omarchy/current/background').resolve(), transition='none')
    elif sys.argv[1:] == ['--next']:
        choices = images()
        if choices:
            current = (HOME_DIR / '.config/omarchy/current/background').resolve()
            index = choices.index(current) if current in choices else -1
            set_wallpaper(choices[(index+1) % len(choices)])
    else:
        raise SystemExit(launch())
