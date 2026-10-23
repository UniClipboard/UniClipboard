#!/usr/bin/env python3
"""Measure synthetic history scrolling in native WebKitGTK on the current display.

Serve frontend/e2e/fixtures/history-scroll.tsx with visual-effects-server.mjs first.
This isolates renderer performance; it does not exercise Wails bindings or a daemon.
Each invocation opens a temporary, unfocused window and exits after the measurement.
"""
import argparse
import json
import os
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--url', default='http://127.0.0.1:1478/')
    parser.add_argument('--dmabuf', choices=('default', 'enabled', 'disabled'), default='default')
    parser.add_argument('--frames', type=int, default=360)
    parser.add_argument('--input', choices=('script', 'touchpad'), default='script',
                        help='touchpad injects synthetic GDK smooth-scroll samples every 8 ms; not physical hardware input')
    parser.add_argument('--page-rate', choices=('default', 'display'), default='default')
    parser.add_argument('--no-smooth-scroll', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if not 60 <= args.frames <= 1200:
        parser.error('--frames must be between 60 and 1200')
    if args.dmabuf != 'default':
        os.environ['WEBKIT_DISABLE_DMABUF_RENDERER'] = '1' if args.dmabuf == 'disabled' else '0'

    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('WebKit2', '4.1')
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gtk, WebKit2, GLib, Gdk

    manager = WebKit2.UserContentManager()
    manager.register_script_message_handler('result')
    window = Gtk.Window(title='UniClipboard synthetic scroll benchmark')
    window.set_default_size(1100, 800)
    window.set_accept_focus(False)
    view = WebKit2.WebView.new_with_user_content_manager(manager)
    window.add(view)
    settings = view.get_settings()
    if args.no_smooth_scroll:
        settings.set_enable_smooth_scrolling(False)
    if args.page_rate == 'display':
        features = WebKit2.Settings.get_all_features()
        for index in range(features.get_length()):
            feature = features.get(index)
            if feature.get_identifier() == 'PreferPageRenderingUpdatesNear60FPS':
                settings.set_feature_enabled(feature, False)
                break
        else:
            raise RuntimeError('WebKit does not expose the page frame rate preference')
    monitor = Gdk.Display.get_default().get_monitor(0)
    refresh_hz = monitor.get_refresh_rate() / 1000
    input_samples = 0
    gtk_frame_times = []
    presentation_feedback = []

    def touchpad_sample():
        nonlocal input_samples
        if input_samples >= args.frames * 2:
            return False
        event = Gdk.Event.new(Gdk.EventType.SCROLL)
        event.window = view.get_window()
        event.send_event = True
        event.time = Gdk.CURRENT_TIME
        event.x, event.y = 300, 300
        event.direction = Gdk.ScrollDirection.SMOOTH
        event.delta_x = 0
        event.delta_y = 0.15 if input_samples < args.frames else -0.15
        event.set_device(Gdk.Display.get_default().get_default_seat().get_pointer())
        view.event(event)
        input_samples += 1
        return True

    results = []
    script = '''(async () => {
      const scroller = document.querySelector('.adw-glist-scroller');
      if (!scroller || scroller.clientHeight < 400 || scroller.scrollHeight < 10000) {
        window.webkit.messageHandlers.result.postMessage(JSON.stringify({ error: 'Fixture has no scrollable history viewport' }));
        return;
      }
      const frames = [];
      let previous = performance.now();
      let mounts = 0;
      const positions = [];
      const observer = new MutationObserver(records => {
        for (const record of records) mounts += record.addedNodes.length;
      });
      observer.observe(scroller, { childList: true, subtree: true });
      for (let index = 0; index < FRAME_COUNT; index++) {
        await new Promise(requestAnimationFrame);
        const now = performance.now();
        frames.push(now - previous);
        previous = now;
        positions.push(scroller.scrollTop);
        if (SCRIPT_INPUT) scroller.scrollTop += index < FRAME_COUNT / 2 ? 18 : -18;
      }
      observer.disconnect();
      frames.sort((a, b) => a - b);
      window.webkit.messageHandlers.result.postMessage(JSON.stringify({
        p50Ms: frames[Math.floor(frames.length * .5)],
        p95Ms: frames[Math.floor(frames.length * .95)],
        maxMs: frames.at(-1),
        over25Ms: frames.filter(ms => ms > 25).length,
        frameCount: frames.length,
        displayFrameBudgetMs: DISPLAY_BUDGET,
        longFrameThresholdMs: DISPLAY_BUDGET * 1.5,
        longFrameCount: frames.filter(ms => ms > DISPLAY_BUDGET * 1.5).length,
        positions,
        mounts,
        mountedItems: document.querySelectorAll('[data-item-id]').length,
        viewportHeight: scroller.clientHeight,
        scrollHeight: scroller.scrollHeight,
        lowEffects: document.documentElement.dataset.ucLowEffects,
        reduceMotion: document.documentElement.dataset.ucReduceMotion,
      }));
    })()'''.replace('FRAME_COUNT', str(args.frames)).replace(
        'SCRIPT_INPUT', 'true' if args.input == 'script' else 'false'
    ).replace('DISPLAY_BUDGET', str(1000 / refresh_hz if refresh_hz else 1000 / 60))

    def result(_manager, message):
        results.append(json.loads(message.get_js_value().to_string()))
        Gtk.main_quit()

    def measure():
        gtk_frame_times.clear()
        presentation_feedback.clear()
        view.evaluate_javascript(script, -1, None, None, None, None, None)
        if args.input == 'touchpad':
            GLib.timeout_add(8, touchpad_sample)
        return False

    def loaded(_view, event):
        if event == WebKit2.LoadEvent.FINISHED:
            GLib.timeout_add(1000, measure)

    def timeout():
        results.append({'error': 'Native scroll measurement timed out'})
        Gtk.main_quit()
        return False

    manager.connect('script-message-received::result', result)
    view.connect('load-changed', loaded)
    view.load_uri(args.url)
    window.show_all()
    clock = view.get_frame_clock()

    def after_paint(frame_clock):
        gtk_frame_times.append(frame_clock.get_frame_time())
        timing = frame_clock.get_current_timings()
        if timing:
            presentation_feedback.append(timing.get_presentation_time())

    # GTK frame clock cadence is distinct from JavaScript callbacks and display presentation.
    clock.connect('after-paint', after_paint)
    GLib.timeout_add_seconds(120, timeout)
    Gtk.main()
    window.destroy()
    gtk_intervals = sorted((b - a) / 1000 for a, b in zip(gtk_frame_times, gtk_frame_times[1:]))
    data = {
        'scope': 'native WebKitGTK, synthetic history; no Wails bindings or daemon',
        'webkitVersion': f'{WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}',
        'dmabufDisabled': os.environ.get('WEBKIT_DISABLE_DMABUF_RENDERER'),
        'displayRefreshHz': refresh_hz,
        'gtkFrameCount': len(gtk_frame_times),
        'gtkFrameP50Ms': gtk_intervals[len(gtk_intervals) // 2] if gtk_intervals else None,
        'gtkFrameP95Ms': gtk_intervals[int(len(gtk_intervals) * .95)] if gtk_intervals else None,
        'gtkPresentationFeedbackCount': sum(t > 0 for t in presentation_feedback),
        'pageRate': args.page_rate,
        'input': args.input,
        'inputSampleCount': input_samples,
        'smoothScrolling': settings.get_enable_smooth_scrolling(),
        'url': args.url,
        'metrics': results[0] if results else {'error': 'No result received'},
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps({**data, 'metrics': {key: value for key, value in data['metrics'].items() if key != 'positions'}}))
    if 'error' in data['metrics']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
