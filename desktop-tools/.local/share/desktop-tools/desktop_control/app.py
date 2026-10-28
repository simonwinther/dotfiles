"""Composition root: wire components once and route typed commands."""
from __future__ import annotations

from .appearance import Appearance
from .applications import Applications
from .capture import Screenshots
from .catalog import ThemeCatalog
from .clipboard import Clipboard
from .commands import Command, CommandName as Name, Dispatcher
from .context import Context
from .controls import Controls
from .menus import Menus
from .preferences import Preferences
from .recording import Recording
from .reminders import Reminders
from .screensavers import Screensavers
from .services import Services
from .session import Session
from .sharing import Sharing
from .storage import StateStore
from .theme_render import ThemeRenderer
from .wallpaper import Wallpaper


class Desktop:
    def __init__(self, ctx: Context) -> None:
        self.ctx = ctx
        self.store = StateStore(ctx.paths)
        self.catalog = ThemeCatalog(ctx, self.store)
        self.renderer = ThemeRenderer(ctx, self.store, self.catalog)
        self.services = Services(ctx)
        self.wallpaper = Wallpaper(ctx, self.store)
        self.appearance = Appearance(ctx, self.store, self.catalog, self.renderer, self.services, self.wallpaper)
        self.applications = Applications(ctx, self.store, self.catalog, self.renderer)
        self.clipboard = Clipboard(ctx, self.applications)
        self.controls = Controls(ctx, self.store, self.services)
        self.preferences = Preferences(ctx, self.store)
        self.session = Session(ctx, self.store, self.renderer, self.appearance, self.controls, self.preferences)
        self.screensavers = Screensavers(ctx, self.store, self.applications)
        self.reminders = Reminders(ctx)
        self.sharing = Sharing(ctx)
        self.screenshots = Screenshots(ctx, self.store, self.clipboard)
        self.recording = Recording(ctx, self.store, self.screenshots)
        self.menus = Menus(ctx, self.store, self.catalog, self.renderer, self.wallpaper, self.reminders, self.dispatch)
        self.dispatcher = Dispatcher({
            Name.THEME: self.appearance.theme, Name.FONT: self.appearance.font,
            Name.APPEARANCE: self.appearance.appearance, Name.WALLPAPER: self.wallpaper.wallpaper,
            Name.TERMINAL: self.applications.terminal, Name.LAUNCH: self.applications.launch,
            Name.RUN: self.applications.run_application, Name.LAUNCHER: self.clipboard.launcher,
            Name.CLIPBOARD: self.clipboard.clipboard, Name.CLIPBOARD_PASTE: self.clipboard.clipboard_paste,
            Name.TOGGLE: self.controls.toggle, Name.DISPLAY: self.controls.display,
            Name.BRIGHTNESS: self.controls.brightness, Name.OSD: self.controls.osd,
            Name.LOCK: self.session.lock, Name.LOCK_WORKER: self.session.lock_worker,
            Name.IDLE_WORKER: self.session.idle_worker, Name.SESSION: self.session.session,
            Name.MONITOR_WATCH: self.session.monitor_watch,
            Name.CAPTURE: self.capture, Name.REMINDER: self.reminders.reminder,
            Name.MENU: self.menus.menu, Name.KEYBINDINGS: lambda _: self.menus.keybindings(),
            Name.TRANSCODE: self.applications.transcode, Name.CONFIG: self.applications.edit_config,
            Name.SHARE: self.sharing.share, Name.ABOUT: self.screensavers.about,
            Name.SCREENSAVER: self.screensavers.screensaver,
            Name.SCREENSAVER_WORKER: self.screensavers.screensaver_worker,
            Name.PREPARE: self.prepare, Name.SEED_PREFERENCES: lambda _: self.preferences.seed_preferences(),
            Name.INTEGRATE: lambda _: self.preferences.integrate(),
            Name.CWD: lambda _: print(self.applications.cwd()), Name.PATH: self.path,
            Name.RESTART: lambda args: self.services.restart(args[0] if args else "theme"),
            Name.WAKE: lambda _: self.controls.wake(), Name.WEATHER: self.applications.weather,
            Name.BATTERY: self.controls.battery, Name.AUDIO: self.controls.audio,
            Name.POWER: self.controls.power, Name.DICTATION_STATUS: self.applications.dictation_status,
            Name.IPHONE_FOLDER: self.applications.iphone_folder, Name.GUIDE: self.applications.guide,
            Name.EXEC: self.applications.execute,
        })

    def dispatch(self, command: Command) -> int:
        return self.dispatcher.dispatch(command)

    def capture(self, args: list[str]) -> int | None:
        if args[:1] == ["record"]:
            return self.recording.recording(args)
        return self.screenshots.capture(args)

    def prepare(self, args: list[str]) -> None:
        source, _, light = self.renderer.prepare_theme()
        self.appearance.apply_appearance(source, light)
        self.preferences.integrate()

    def path(self, args: list[str]) -> None:
        if len(args) != 1:
            raise ValueError("path requires a tool name")
        print(self.ctx.executable(args[0]))
