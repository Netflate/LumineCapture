import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Clutter from 'gi://Clutter';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BUS_NAME = 'io.github.netflate.LumineCapture.Shell';
const OBJECT_PATH = '/io/github/netflate/LumineCapture/Shell';
const INTERFACE_NAME = 'io.github.netflate.LumineCapture.Shell1';
const ERROR_PREFIX = `${INTERFACE_NAME}.Error.`;
const XML = `<node>
    <interface name="${INTERFACE_NAME}">
        <method name="GetFocusWindow">
            <arg name="id" type="t" direction="out" />
            <arg name="frame" type="(iiii)" direction="out" />
            <arg name="buffer" type="(iiii)" direction="out" />
        </method>
        <method name="KeepAbove" />
        <method name="Passthrough" />
        <method name="ClaimRecording" />
    </interface>
</node>`;

function rectTuple(rect) {
    return [rect.x, rect.y, rect.width, rect.height];
}

export default class LumineCaptureExtension extends Extension {
    enable() {
        this._passthrough = new Map();
        this._claimUntil = 0;
        this._ownerId = Gio.bus_own_name_on_connection(Gio.DBus.session, BUS_NAME,
            Gio.BusNameOwnerFlags.NONE, null, null);
        this._exported = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._exported.export(Gio.DBus.session, OBJECT_PATH);
        this._controller = global.backend.get_remote_access_controller();
        // The shell's applet may connect later than us, so it must run after it
        // has added the handle
        this._newHandleId = this._controller.connect_after('new-handle', (_controller, handle) => {
            if (!handle.is_recording || GLib.get_monotonic_time() > this._claimUntil)
                return;
            this._claimUntil = 0;
            const applet = Main.panel.statusArea.quickSettings._remoteAccess;
            applet._handles.delete(handle);
            applet._sync();
        });
    }

    disable() {
        for (const actor of [...this._passthrough.keys()])
            this._restore(actor);
        this._controller.disconnect(this._newHandleId);
        this._controller = null;
        this._exported?.unexport();
        this._exported = null;
        if (this._ownerId)
            Gio.bus_unown_name(this._ownerId);
        this._ownerId = 0;
    }

    GetFocusWindowAsync(_params, invocation) {
        const window = global.display.focus_window;
        if (!window)
            return this._error(invocation, 'NoWindow', 'There is no focused window');
        invocation.return_value(new GLib.Variant('(t(iiii)(iiii))', [
            window.get_id(), rectTuple(window.get_frame_rect()), rectTuple(window.get_buffer_rect()),
        ]));
    }

    async KeepAboveAsync(_params, invocation) {
        try {
            const pid = await this._callerPid(invocation);
            for (const window of this._windowsOf(pid))
                window.make_above();
            invocation.return_value(null);
        } catch (error) {
            this._error(invocation, 'Failed', error.message);
        }
    }

    async PassthroughAsync(_params, invocation) {
        try {
            const pid = await this._callerPid(invocation);
            for (const window of this._windowsOf(pid)) {
                const actor = window.get_compositor_private();
                if (!actor)
                    continue;
                for (const child of actor.get_children()) {
                    if (child.constructor === Clutter.Actor)
                        child.hide();
                }
                // funny workaround
                // Mutter puts a black actor under translucent fullscreen windows. Hiding it isn't enough,
                // Mutter still skips drawing what's under the window while (mutter 50,
                // meta-window-actor-wayland.c, subtract_background_opaque_region):
                //     self->background && clutter_actor_get_paint_opacity (self) == 0xff
                // so 254 breaks the second half. And we can't hide the black thing, mutter needs it.
                actor.opacity = 254;
                if (!this._passthrough.has(actor))
                    this._passthrough.set(actor, actor.connect('destroy', () => {
                        this._passthrough.delete(actor);
                    }));
            }
            invocation.return_value(null);
        } catch (error) {
            this._error(invocation, 'Failed', error.message);
        }
    }

    async ClaimRecordingAsync(_params, invocation) {
        try {
            const pid = await this._callerPid(invocation);
            const executable = GLib.file_read_link(`/proc/${pid}/exe`);
            const info = Gio.File.new_for_path(executable).query_info(
                'unix::uid', Gio.FileQueryInfoFlags.NONE, null);

            // security
            if (GLib.path_get_basename(executable) !== 'lumine-capture' ||
                info.get_attribute_uint32('unix::uid') !== 0)
                return this._error(
                    invocation, 'NotAllowed',
                    'The caller is not the packaged lumine-capture binary');

          this._claimUntil = GLib.get_monotonic_time() + 1000 * 1000;
            invocation.return_value(null);
        } catch (error) {
            this._error(invocation, 'Failed', error.message);
        }
    }

    _callerPid(invocation) {
        return new Promise((resolve, reject) => {
            Gio.DBus.session.call(
                'org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus', 'GetConnectionUnixProcessID',
                new GLib.Variant('(s)', [invocation.get_sender()]),
                new GLib.VariantType('(u)'),
                Gio.DBusCallFlags.NONE, -1, null, (connection, result) => {
                    try {
                        resolve(connection.call_finish(result).deep_unpack()[0]);
                    } catch (error) {
                        reject(error);
                    }
                });
        });
    }

    _restore(actor) {
        for (const child of actor.get_children()) {
            if (child.constructor === Clutter.Actor)
                child.show();
        }
        actor.opacity = 255;
        const id = this._passthrough.get(actor);
        if (id)
            actor.disconnect(id);
        this._passthrough.delete(actor);
    }

    _windowsOf(pid) {
        return global.display.list_all_windows().filter(window => window.get_pid() === pid);
    }

    _error(invocation, name, message) {
        invocation.return_dbus_error(`${ERROR_PREFIX}${name}`, message);
    }
}
