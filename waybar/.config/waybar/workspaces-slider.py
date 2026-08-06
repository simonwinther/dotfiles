#!/usr/bin/env python3
"""Animated Hyprland workspace pill for Waybar's center position."""

import json
import math
import os
import socket
import subprocess
import threading
import time

import cairo
import gi

gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
gi.require_version("GtkLayerShell", "0.1")
gi.require_version("Pango", "1.0")
gi.require_version("PangoCairo", "1.0")

from gi.repository import Gdk, GLib, Gtk, GtkLayerShell, Pango, PangoCairo


WORKSPACE_COUNT = 10
PANEL_WIDTH = 292
PANEL_HEIGHT = 34
PANEL_TOP = 14
SLOT_WIDTH = 28
FIRST_CENTER = 20
INDICATOR_RADIUS = 13

BASE = (0x1E / 255, 0x1E / 255, 0x2E / 255)
TEXT = (0xCD / 255, 0xD6 / 255, 0xF4 / 255)
YELLOW = (0xF9 / 255, 0xE2 / 255, 0xAF / 255)
BLUE = (0x89 / 255, 0xB4 / 255, 0xFA / 255)
SURFACE2 = (0x58 / 255, 0x5B / 255, 0x70 / 255)


def hypr_json(*arguments):
    try:
        result = subprocess.run(
            ["hyprctl", *arguments, "-j"],
            check=True,
            capture_output=True,
            text=True,
            timeout=1,
        )
        return json.loads(result.stdout)
    except (OSError, subprocess.SubprocessError, json.JSONDecodeError):
        return None


def monitor_snapshot(monitor_name):
    monitors = hypr_json("monitors") or []
    monitor = next((item for item in monitors if item.get("name") == monitor_name), None)
    if monitor is None:
        return None

    active = monitor.get("activeWorkspace", {}).get("id", 1)
    workspaces = hypr_json("workspaces") or []
    occupied = {
        workspace.get("id")
        for workspace in workspaces
        if workspace.get("monitor") == monitor_name
        and 1 <= workspace.get("id", 0) <= WORKSPACE_COUNT
    }
    return monitor, active, occupied


def rounded_rectangle(context, x, y, width, height, radius):
    context.new_sub_path()
    context.arc(x + width - radius, y + radius, radius, -math.pi / 2, 0)
    context.arc(x + width - radius, y + height - radius, radius, 0, math.pi / 2)
    context.arc(x + radius, y + height - radius, radius, math.pi / 2, math.pi)
    context.arc(x + radius, y + radius, radius, math.pi, 3 * math.pi / 2)
    context.close_path()


class WorkspaceSlider(Gtk.DrawingArea):
    def __init__(self, monitor_name, active, occupied):
        super().__init__()
        self.monitor_name = monitor_name
        self.set_size_request(PANEL_WIDTH, PANEL_HEIGHT)
        self.add_events(Gdk.EventMask.BUTTON_PRESS_MASK | Gdk.EventMask.SCROLL_MASK)
        self.connect("draw", self.draw)
        self.connect("button-press-event", self.click)
        self.connect("scroll-event", self.scroll)

        self.active = active if 1 <= active <= WORKSPACE_COUNT else 1
        self.position = self.center_for(self.active)
        self.start_position = self.position
        self.target_position = self.position
        self.animation_started = 0.0
        self.animation_duration = 0.20
        self.occupied = occupied
        self.deferred_occupied = None
        self.refresh_source = None

        GLib.timeout_add(16, self.animate)
        GLib.timeout_add_seconds(2, self.fallback_refresh)
        threading.Thread(target=self.listen_to_hyprland, daemon=True).start()

    @staticmethod
    def center_for(workspace):
        return FIRST_CENTER + (workspace - 1) * SLOT_WIDTH

    def set_state(self, workspace, occupied):
        previous_occupied = self.occupied
        changed = occupied != previous_occupied
        self.occupied = occupied

        if 1 <= workspace <= WORKSPACE_COUNT and workspace != self.active:
            self.deferred_occupied = workspace if workspace not in previous_occupied else None
            self.active = workspace
            self.start_position = self.position
            self.target_position = self.center_for(workspace)
            distance = abs(self.target_position - self.start_position) / SLOT_WIDTH
            self.animation_duration = min(0.30, 0.17 + distance * 0.018)
            self.animation_started = time.monotonic()
            changed = True

        if changed:
            self.queue_draw()
        return False

    def refresh_state(self):
        self.refresh_source = None
        snapshot = monitor_snapshot(self.monitor_name)
        if snapshot is not None:
            _monitor, active, occupied = snapshot
            self.set_state(active, occupied)
        return False

    def schedule_refresh(self):
        if self.refresh_source is None:
            self.refresh_source = GLib.timeout_add(8, self.refresh_state)
        return False

    def fallback_refresh(self):
        self.refresh_state()
        return True

    def listen_to_hyprland(self):
        runtime = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
        signature = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE", "")
        socket_path = os.path.join(runtime, "hypr", signature, ".socket2.sock")

        while True:
            try:
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                    connection.connect(socket_path)
                    stream = connection.makefile("r", encoding="utf-8")
                    for line in stream:
                        event, _, _payload = line.strip().partition(">>")
                        if event in {
                            "workspace",
                            "workspacev2",
                            "createworkspace",
                            "createworkspacev2",
                            "destroyworkspace",
                            "destroyworkspacev2",
                            "moveworkspace",
                            "moveworkspacev2",
                            "focusedmon",
                            "focusedmonv2",
                            "monitoradded",
                            "monitoraddedv2",
                            "monitorremoved",
                        }:
                            GLib.idle_add(self.schedule_refresh)
            except OSError:
                time.sleep(1)

    def animate(self):
        if self.position == self.target_position:
            return True

        elapsed = time.monotonic() - self.animation_started
        progress = min(1.0, elapsed / self.animation_duration)
        eased = 1 - pow(1 - progress, 3)
        self.position = self.start_position + (self.target_position - self.start_position) * eased
        if progress >= 1:
            self.position = self.target_position
            self.deferred_occupied = None
        self.queue_draw()
        return True

    @staticmethod
    def set_source(context, color, alpha=1.0):
        context.set_source_rgba(*color, alpha)

    def draw_labels(self, context, color_override=None):
        layout = PangoCairo.create_layout(context)
        font = Pango.FontDescription("JetBrainsMono Nerd Font Bold")
        font.set_absolute_size(14 * Pango.SCALE)
        layout.set_font_description(font)

        for workspace in range(1, WORKSPACE_COUNT + 1):
            label = "0" if workspace == 10 else str(workspace)
            layout.set_text(label, -1)
            _, logical = layout.get_pixel_extents()
            x = self.center_for(workspace) - logical.width / 2 - logical.x
            y = PANEL_HEIGHT / 2 - logical.height / 2 - logical.y
            occupied = workspace in self.occupied and workspace != self.deferred_occupied
            color = color_override or (YELLOW if occupied else SURFACE2)
            self.set_source(context, color)
            context.move_to(x, y)
            PangoCairo.show_layout(context, layout)

    def draw(self, _widget, context):
        context.set_operator(cairo.OPERATOR_SOURCE)
        context.set_source_rgba(0, 0, 0, 0)
        context.paint()
        context.set_operator(cairo.OPERATOR_OVER)

        rounded_rectangle(context, 0, 0, PANEL_WIDTH, PANEL_HEIGHT, PANEL_HEIGHT / 2)
        self.set_source(context, BASE, 0.96)
        context.fill()

        rounded_rectangle(context, 0.5, 0.5, PANEL_WIDTH - 1, PANEL_HEIGHT - 1, 16.5)
        self.set_source(context, SURFACE2, 0.22)
        context.set_line_width(1)
        context.stroke()

        self.draw_labels(context)

        for radius, alpha in ((17, 0.035), (15, 0.07)):
            context.arc(self.position, PANEL_HEIGHT / 2 + 2, radius, 0, 2 * math.pi)
            self.set_source(context, BLUE, alpha)
            context.fill()

        context.arc(self.position, PANEL_HEIGHT / 2, INDICATOR_RADIUS, 0, 2 * math.pi)
        self.set_source(context, BLUE)
        context.fill()
        context.arc(self.position, PANEL_HEIGHT / 2, INDICATOR_RADIUS - 0.5, 0, 2 * math.pi)
        self.set_source(context, TEXT, 0.20)
        context.set_line_width(1)
        context.stroke()

        context.save()
        context.arc(self.position, PANEL_HEIGHT / 2, INDICATOR_RADIUS - 0.5, 0, 2 * math.pi)
        context.clip()
        self.draw_labels(context, BASE)
        context.restore()
        return False

    def workspace_at(self, x):
        workspace = round((x - FIRST_CENTER) / SLOT_WIDTH) + 1
        return max(1, min(WORKSPACE_COUNT, workspace))

    def click(self, _widget, event):
        workspace = self.workspace_at(event.x)
        subprocess.Popen(
            [
                "hyprctl",
                "--batch",
                f"dispatch focusmonitor {self.monitor_name} ; dispatch workspace {workspace}",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return True

    def scroll(self, _widget, event):
        direction = "e+1" if event.direction == Gdk.ScrollDirection.DOWN else "e-1"
        subprocess.Popen(
            [
                "hyprctl",
                "--batch",
                f"dispatch focusmonitor {self.monitor_name} ; dispatch workspace {direction}",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return True


def main():
    monitors = hypr_json("monitors") or []
    monitor_name = os.environ.get("WAYBAR_OUTPUT_NAME")
    if not monitor_name:
        focused = next((monitor for monitor in monitors if monitor.get("focused")), None)
        monitor_name = focused.get("name") if focused else None

    snapshot = monitor_snapshot(monitor_name) if monitor_name else None
    if snapshot is None:
        return
    monitor, active, occupied = snapshot

    window = Gtk.Window(type=Gtk.WindowType.TOPLEVEL)
    window.set_title("Waybar workspace slider")
    window.set_decorated(False)
    window.set_app_paintable(True)
    visual = window.get_screen().get_rgba_visual()
    if visual:
        window.set_visual(visual)

    GtkLayerShell.init_for_window(window)
    GtkLayerShell.set_namespace(window, f"waybar-workspace-slider-{monitor_name}")
    GtkLayerShell.set_layer(window, GtkLayerShell.Layer.BOTTOM)
    display = Gdk.Display.get_default()
    gdk_monitor = display.get_monitor_at_point(monitor.get("x", 0) + 1, monitor.get("y", 0) + 1)
    if gdk_monitor is not None:
        GtkLayerShell.set_monitor(window, gdk_monitor)
    GtkLayerShell.set_anchor(window, GtkLayerShell.Edge.TOP, True)
    GtkLayerShell.set_margin(window, GtkLayerShell.Edge.TOP, PANEL_TOP)
    # Draw over Waybar's reserved strip instead of below it.
    GtkLayerShell.set_exclusive_zone(window, -1)
    GtkLayerShell.set_keyboard_interactivity(window, False)

    window.add(WorkspaceSlider(monitor_name, active, occupied))
    window.connect("destroy", Gtk.main_quit)
    window.show_all()
    Gtk.main()


if __name__ == "__main__":
    main()
