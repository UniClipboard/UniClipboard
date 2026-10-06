# Headless Linux build and verification image for the Go GUI (slice 17c).
# It proves the Linux code compiles and runs under Xvfb + a private D-Bus session. It is NOT a real desktop:
# there is no Wayland compositor, no GlobalShortcuts portal, no tray host and no Secret Service unless a test adds one.
FROM ubuntu:24.04
ARG GO_VERSION=1.27.1
ARG TARGETARCH
ENV DEBIAN_FRONTEND=noninteractive
# The mirror drops requests now and then; retrying inside the same layer keeps the packages already downloaded.
RUN apt-get update && for i in 1 2 3 4 5 6; do \
      apt-get install -y --no-install-recommends -o Acquire::Retries=5 --fix-missing \
        ca-certificates curl git build-essential pkg-config xz-utils \
        libgtk-3-dev libwebkit2gtk-4.1-dev libx11-dev libdbus-1-dev libssl-dev \
        xvfb xauth dbus dbus-x11 xdotool x11-utils python3 && break; \
      echo "apt attempt $i failed" >&2; sleep 5; apt-get update; \
    done && command -v Xvfb && rm -rf /var/lib/apt/lists/*
# Strict repair and completion: the layer above is kept for its downloaded work, but it is NOT trusted (its loop could
# finish with every attempt failed). This re-runs the same set with failure propagation, configures anything left
# unconfigured, then installs the packaging tools the same way. APT_OPTS adds transport options for the host's
# download path (see the comments in apt_install.sh and README "Linux (17c)").
COPY apt_install.sh verify_image.sh /usr/local/bin/
ARG APT_OPTS=""
RUN APT_OPTS="$APT_OPTS" apt_install.sh \
      ca-certificates curl git build-essential pkg-config xz-utils \
      libgtk-3-dev libwebkit2gtk-4.1-dev libx11-dev libdbus-1-dev libssl-dev \
      xvfb xauth dbus dbus-x11 xdotool x11-utils python3
# Packaging tools (package_linux.py): rpmbuild for the rpm, file for inspection. dpkg-deb ships with the base image.
RUN APT_OPTS="$APT_OPTS" apt_install.sh rpm file desktop-file-utils
RUN curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-${TARGETARCH}.tar.gz" | tar -C /usr/local -xz
ENV PATH=/usr/local/go/bin:$PATH GOFLAGS=-mod=mod GOTOOLCHAIN=local
RUN verify_image.sh
