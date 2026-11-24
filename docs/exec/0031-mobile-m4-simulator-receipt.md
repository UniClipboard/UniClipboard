# 0031 M04 simulator preflight

The build comparison uses source `4f29e176b` and the M04 change to the
mobile binding script. The app runs used an arm64 iPhone 17 Pro Simulator
on iOS 26.3 and an arm64 Pixel 8 emulator on Android API 36. Both were
disposable instances. The previous and optimized Release app builds used
the same source and the matching sets of Go bindings.

## Build evidence

| Artifact | Previous | Optimized |
| --- | ---: | ---: |
| iOS device arm64 Go archive | 106.30 MB | 51.49 MB |
| iOS dual-architecture Simulator Go archive | 209.15 MB | 99.98 MB |
| Android dual-architecture Go AAR | 58.13 MB | 31.85 MB |
| Android arm64 Release APK | 103.78 MB | 103.66 MB |
| iOS arm64 Release Simulator app, local ZIP | 36.00 MB | 35.35 MB |

The binding build uses `-trimpath -ldflags='-s -w'`. The optimized iOS
Simulator executable and its dSYM have matching UUIDs. The 89.39 MB
Android Release AAB contains both arm64-v8a and x86_64 libraries and
native symbol metadata. The AAB uses the local development signing
configuration. Record store-estimated per-ABI download size separately.

On the baseline Android arm64 Go library, `go tool nm -size` attributed
2.62 MB of text/data/BSS symbol sizes to `tailscale.com/` names and
2.33 MB to `modernc.org/` names. These symbol sums provide a comparison
by prefix. They can overlap and do not partition the file on disk.

## Simulator evidence

| Scenario | Result |
| --- | --- |
| R01 setup and restart | iOS baseline and optimized Android passed fresh Setup, persisted `complete:true`, and returned to the live app after restart. |
| R02 compatible backup | Optimized iOS passed the partial-import flow with a synthetic encrypted backup. Independent SQLite readback found two text records, one image and one file. |
| 500-record history | Both optimized apps opened the last item and presented its detail. The profile generator wrote 500 synthetic items per platform. |
| R07 Android setting persistence | The optimized app retained an edited device name after a force-stop. Both the settings page and the stored settings confirmed the exact name. |
| R08 Android text intake | The optimized app accepted a synthetic `ACTION_SEND` text intent. The saved payload occurred exactly once in the local database. |

Android `am start -W` reported a cold activity `TotalTime` of 869 ms for
the 500-record profile. This timestamp measures the activity launch.
RSS samples taken after entering detail were 343232 KiB on the iOS host
and 293684 KiB in the Android guest. The host and guest accounting and
capture state differ. The measurements describe the tested simulator runs.

The first Android R01 run timed out. Its failure hierarchy showed the
Setup button at capture time. An extended startup wait passed on the
optimized APK. The first Android share launch lost its shell quotes.
The retry passed with the intent payload quoted for the remote shell. The
first device-name run started in item detail. After relaunch, the next run
retained a suffix from the previous field value. Starting on the history
tab and placing the cursor at the end before erasing passed the retry.

The physical-device R01–R08 gate remains open. The store download
measurement, 500-record time to interactive on hardware, idle RSS,
100 MB transfer RSS peak, and WAL recovery during an active write remain
pending. The evidence archive includes source identity, artifact hashes,
fixture generation and staging scripts, flow reports, and readback results.
