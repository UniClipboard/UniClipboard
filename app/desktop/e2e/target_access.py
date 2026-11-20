"""Read focus and inserted text from exactly one task-owned editor process."""
import sys
import pyatspi

pid, mode = int(sys.argv[1]), sys.argv[2]
applications = [app for app in pyatspi.Registry.getDesktop(0) if app.get_process_id() == pid]
if len(applications) != 1:
    raise SystemExit(1)
frames = list(applications[0])
if not any(frame.getState().contains(pyatspi.STATE_ACTIVE) for frame in frames):
    raise SystemExit(1)
if mode == 'active':
    raise SystemExit(0)
if mode != 'text':
    raise SystemExit(2)
queue, text = frames[:], []
for _ in range(1000):
    if not queue:
        break
    node = queue.pop()
    state = node.getState()
    if state.contains(pyatspi.STATE_EDITABLE) and state.contains(pyatspi.STATE_SHOWING):
        try:
            text.append(node.queryText().getText(0, -1))
        except NotImplementedError:
            pass
    queue.extend(node)
if len(text) != 1:
    raise SystemExit(1)
sys.stdout.write(text[0])
