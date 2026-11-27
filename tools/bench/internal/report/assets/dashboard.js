(function () {
  "use strict";

  var SVG_NS = "http://www.w3.org/2000/svg";
  var SERIES_COLORS = ["#2563eb", "#0d9488", "#7c3aed", "#d97706", "#db2777", "#4f46e5"];
  var ROLE_COLORS = {
    "unicd": "#2563eb",
    "tray": "#0d9488",
    "quick-panel": "#d97706",
    "gui": "#7c3aed",
    "launcher": "#64748b"
  };

  var rawEl = document.getElementById("unic-bench-data");
  if (!rawEl) {
    return;
  }
  var payload;
  try {
    payload = JSON.parse(rawEl.textContent || "{}");
  } catch (err) {
    renderFatalError("Failed to parse embedded benchmark JSON: " + String(err));
    return;
  }

  var reports = Array.isArray(payload.reports) ? payload.reports : [];
  var state = {
    theme: "light",
    selectedReports: new Set(reports.map(function (r) { return r.index; })),
    track: "all",
    dataset: "all",
    attrReportIdx: reports.length > 0 ? reports[0].index : 0,
    attrScenarioKey: "",
    attrRole: ""
  };

  var tooltipEl = document.getElementById("bench-tooltip");

  function renderFatalError(msg) {
    var main = document.querySelector("main.container");
    if (!main) {
      return;
    }
    var box = el("div", "banner banner-danger");
    box.appendChild(el("div", "banner-title", msg));
    main.replaceChildren(box);
  }

  function el(tag, className, text) {
    var node = document.createElement(tag);
    if (className) {
      node.className = className;
    }
    if (text !== undefined && text !== null) {
      node.textContent = String(text);
    }
    return node;
  }

  function svgEl(tag, attrs, text) {
    var node = document.createElementNS(SVG_NS, tag);
    if (attrs) {
      Object.keys(attrs).forEach(function (k) {
        if (attrs[k] !== undefined && attrs[k] !== null) {
          node.setAttribute(k, String(attrs[k]));
        }
      });
    }
    if (text !== undefined && text !== null) {
      node.textContent = String(text);
    }
    return node;
  }

  function numOrZero(v) {
    return typeof v === "number" && Number.isFinite(v) ? v : 0;
  }

  function isFiniteNum(v) {
    return typeof v === "number" && Number.isFinite(v);
  }

  function formatBytes(v) {
    if (!isFiniteNum(v)) {
      return "—";
    }
    var abs = Math.abs(v);
    var sign = v < 0 ? "-" : "";
    var kib = 1024;
    var mib = 1024 * 1024;
    var gib = 1024 * 1024 * 1024;
    if (abs >= gib) {
      return sign + (abs / gib).toFixed(2) + " GiB";
    }
    if (abs >= mib) {
      return sign + (abs / mib).toFixed(2) + " MiB";
    }
    if (abs >= kib) {
      return sign + (abs / kib).toFixed(1) + " KiB";
    }
    return sign + Math.round(abs) + " B";
  }

  function formatSignedBytes(v) {
    if (!isFiniteNum(v)) {
      return "—";
    }
    if (v > 0) {
      return "+" + formatBytes(v);
    }
    return formatBytes(v);
  }

  function formatMiB(v) {
    if (!isFiniteNum(v)) {
      return "—";
    }
    return (v / (1024 * 1024)).toFixed(2) + " MiB";
  }

  function formatSlopeMiB(v) {
    if (!isFiniteNum(v)) {
      return "—";
    }
    var mib = v / (1024 * 1024);
    var prefix = mib > 0 ? "+" : "";
    return prefix + mib.toFixed(2) + " MiB/round";
  }

  function formatPct(v) {
    if (!isFiniteNum(v)) {
      return "—";
    }
    return v.toFixed(2) + "%";
  }

  function formatMs(v) {
    if (!isFiniteNum(v)) {
      return "—";
    }
    return v.toFixed(1) + " ms";
  }

  function reportColor(idx) {
    return SERIES_COLORS[idx % SERIES_COLORS.length];
  }

  function roleColor(role) {
    return ROLE_COLORS[role] || "#475569";
  }

  function showTooltip(evt, title, lines) {
    if (!tooltipEl) {
      return;
    }
    tooltipEl.replaceChildren();
    tooltipEl.appendChild(el("div", "bench-tooltip-title", title));
    (lines || []).forEach(function (line) {
      tooltipEl.appendChild(el("div", "bench-tooltip-line", line));
    });
    tooltipEl.hidden = false;
    moveTooltip(evt);
  }

  function moveTooltip(evt) {
    if (!tooltipEl || tooltipEl.hidden) {
      return;
    }
    var x = (evt && typeof evt.clientX === "number" ? evt.clientX : 20) + 12;
    var y = (evt && typeof evt.clientY === "number" ? evt.clientY : 20) + 12;
    var maxW = window.innerWidth - 340;
    var maxH = window.innerHeight - 140;
    tooltipEl.style.left = Math.max(8, Math.min(x, maxW)) + "px";
    tooltipEl.style.top = Math.max(8, Math.min(y, maxH)) + "px";
  }

  function hideTooltip() {
    if (tooltipEl) {
      tooltipEl.hidden = true;
    }
  }

  function bindTooltip(node, title, lines) {
    node.setAttribute("tabindex", "0");
    node.addEventListener("mouseenter", function (e) { showTooltip(e, title, lines); });
    node.addEventListener("mousemove", moveTooltip);
    node.addEventListener("mouseleave", hideTooltip);
    node.addEventListener("focus", function () {
      var rect = node.getBoundingClientRect();
      showTooltip({ clientX: rect.left, clientY: rect.bottom }, title, lines);
    });
    node.addEventListener("blur", hideTooltip);
  }

  function getActiveReports() {
    return reports.filter(function (r) { return state.selectedReports.has(r.index); });
  }

  function scenarioTrack(sc, repEntry) {
    return sc.track || (repEntry.report && repEntry.report.track) || "";
  }

  function scenarioDataset(sc) {
    return sc.dataset || sc.preset || "";
  }

  function scenarioMatches(sc, repEntry) {
    var tr = scenarioTrack(sc, repEntry);
    var ds = scenarioDataset(sc);
    if (state.track !== "all" && tr !== state.track) {
      return false;
    }
    if (state.dataset !== "all" && ds !== state.dataset) {
      return false;
    }
    return true;
  }

  function findScenarios(repEntry, id) {
    var list = (repEntry.report && Array.isArray(repEntry.report.scenarios)) ? repEntry.report.scenarios : [];
    return list.filter(function (sc) {
      return sc.id === id && scenarioMatches(sc, repEntry);
    });
  }

  function isMetricAvailable(osName, metricKey, statsOrVal) {
    if (metricKey === "rss_bytes") {
      return statsOrVal && numOrZero(statsOrVal.count) > 0;
    }
    if (metricKey === "private_commit_bytes" || metricKey === "working_set_bytes") {
      return statsOrVal && numOrZero(statsOrVal.count) > 0;
    }
    if (metricKey === "idle_wakeups_per_sec" || metricKey === "fds_max") {
      return osName === "darwin" || numOrZero(statsOrVal) > 0;
    }
    if (metricKey === "handles_max" || metricKey === "gdi_objects_max" || metricKey === "user_objects_max") {
      return osName === "windows" || numOrZero(statsOrVal) > 0;
    }
    return isFiniteNum(statsOrVal);
  }

  function initToolbar() {
    var metaBadge = document.getElementById("report-meta-badge");
    if (metaBadge) {
      var parts = [reports.length + " report(s)"];
      if (payload.cross_platform) {
        parts.push("Cross-Platform Side-by-Side");
      }
      if (payload.generated_at_utc) {
        parts.push(payload.generated_at_utc);
      }
      metaBadge.textContent = parts.join(" · ");
    }

    var themeBtn = document.getElementById("theme-toggle");
    if (themeBtn) {
      themeBtn.addEventListener("click", function () {
        state.theme = state.theme === "light" ? "dark" : "light";
        document.documentElement.setAttribute("data-theme", state.theme);
        themeBtn.textContent = state.theme === "light" ? "Dark Mode" : "Light Mode";
      });
    }

    var reportPills = document.getElementById("report-pills");
    if (reportPills) {
      reportPills.replaceChildren();
      reports.forEach(function (r) {
        var btn = el("button", "pill active", r.label + " (" + r.platform + ")");
        btn.type = "button";
        btn.addEventListener("click", function () {
          if (state.selectedReports.has(r.index) && state.selectedReports.size > 1) {
            state.selectedReports.delete(r.index);
            btn.classList.remove("active");
          } else {
            state.selectedReports.add(r.index);
            btn.classList.add("active");
          }
          renderAll();
        });
        reportPills.appendChild(btn);
      });
    }

    var trackButtons = document.querySelectorAll("#track-pills .pill");
    trackButtons.forEach(function (btn) {
      btn.addEventListener("click", function () {
        state.track = btn.getAttribute("data-track") || "all";
        trackButtons.forEach(function (b) {
          b.classList.toggle("active", b === btn);
        });
        renderAll();
      });
    });

    var datasets = new Set();
    reports.forEach(function (r) {
      var scs = (r.report && Array.isArray(r.report.scenarios)) ? r.report.scenarios : [];
      scs.forEach(function (sc) {
        var ds = scenarioDataset(sc);
        if (ds) {
          datasets.add(ds);
        }
      });
    });
    var dsPills = document.getElementById("dataset-pills");
    if (dsPills) {
      dsPills.replaceChildren();
      var allBtn = el("button", "pill active", "All");
      allBtn.type = "button";
      allBtn.addEventListener("click", function () {
        state.dataset = "all";
        updateDatasetPills(dsPills, allBtn);
        renderAll();
      });
      dsPills.appendChild(allBtn);

      Array.from(datasets).sort().forEach(function (ds) {
        var b = el("button", "pill", ds);
        b.type = "button";
        b.addEventListener("click", function () {
          state.dataset = ds;
          updateDatasetPills(dsPills, b);
          renderAll();
        });
        dsPills.appendChild(b);
      });
    }
  }

  function updateDatasetPills(container, activeBtn) {
    container.querySelectorAll(".pill").forEach(function (b) {
      b.classList.toggle("active", b === activeBtn);
    });
  }

  function syncHeaderScrollPadding() {
    var topbar = document.querySelector(".topbar");
    if (!topbar) {
      return;
    }
    var h = Math.ceil(topbar.getBoundingClientRect().height + 16);
    document.documentElement.style.scrollPaddingTop = h + "px";
    document.querySelectorAll("main section[id]").forEach(function (sec) {
      sec.style.scrollMarginTop = h + "px";
    });
  }

  function wrapAxisLines(text, maxChars) {
    var str = String(text || "");
    if (str.length <= maxChars) {
      return [str];
    }
    var tokens = str.split(/(\s+·\s+|\s+|\/|-|_)/).filter(function (t) { return t.length > 0; });
    var lines = [];
    var cur = "";
    tokens.forEach(function (tok) {
      if (/^\s*·\s*$/.test(tok)) {
        if (cur) {
          lines.push(cur.replace(/\s+$/, ""));
          cur = "";
        }
        return;
      }
      var candidate = cur ? (cur + tok) : tok.replace(/^\s+/, "");
      if (candidate.length <= maxChars || !cur) {
        cur = candidate;
      } else {
        lines.push(cur.replace(/\s+$/, ""));
        cur = tok.replace(/^\s+/, "");
      }
    });
    if (cur.replace(/\s+$/, "")) {
      lines.push(cur.replace(/\s+$/, ""));
    }
    return lines.length > 0 ? lines : [str];
  }

  function appendWrappedAxisLabel(svg, x, baseY, lines) {
    var txt = svgEl("text", {
      "x": x.toFixed(1),
      "y": baseY.toFixed(1),
      "text-anchor": "middle",
      "class": "svg-axis-label"
    });
    lines.forEach(function (line, i) {
      txt.appendChild(svgEl("tspan", {
        "x": x.toFixed(1),
        "dy": i === 0 ? "0" : "13"
      }, line));
    });
    svg.appendChild(txt);
  }

  function renderStackedBarChart(container, config) {
    /* config: { title, note, items: [{ label, sublabel, segments: [{ key, value, color }], markerValue, markerLabel }], valueFormatter } */
    var panel = el("div", "chart-panel");
    var head = el("div", "chart-header");
    head.appendChild(el("h3", "chart-title", config.title));
    if (config.note) {
      head.appendChild(el("span", "chart-note", config.note));
    }
    panel.appendChild(head);

    if (!config.items || config.items.length === 0) {
      panel.appendChild(el("div", "empty-state", "No matching data for stacked chart."));
      container.appendChild(panel);
      return;
    }

    var legendKeys = [];
    var legendMap = {};
    config.items.forEach(function (it) {
      (it.segments || []).forEach(function (seg) {
        if (!legendMap[seg.key]) {
          legendMap[seg.key] = seg.color;
          legendKeys.push(seg.key);
        }
      });
    });
    if (legendKeys.length > 0) {
      var leg = el("div", "legend-row");
      legendKeys.forEach(function (k) {
        var li = el("span", "legend-item");
        var sw = el("span", "legend-swatch");
        sw.style.backgroundColor = legendMap[k];
        li.appendChild(sw);
        li.appendChild(document.createTextNode(k));
        leg.appendChild(li);
      });
      if (config.hasMarker) {
        var ml = el("span", "legend-item");
        ml.appendChild(el("span", "badge badge-warn", "◆ EarlyPeak"));
        leg.appendChild(ml);
      }
      panel.appendChild(leg);
    }

    var maxVal = 0;
    config.items.forEach(function (it) {
      var sum = 0;
      (it.segments || []).forEach(function (seg) {
        sum += Math.max(0, numOrZero(seg.value));
      });
      if (sum > maxVal) {
        maxVal = sum;
      }
      if (isFiniteNum(it.markerValue) && it.markerValue > maxVal) {
        maxVal = it.markerValue;
      }
    });
    if (maxVal <= 0) {
      maxVal = 1;
    }

    var wrappedLabels = config.items.map(function (it) {
      var lines = wrapAxisLines(it.label, 18);
      if (it.sublabel) {
        lines = lines.concat(wrapAxisLines(it.sublabel, 18));
      }
      return lines;
    });
    var maxLines = 1;
    var maxLineChars = 8;
    wrappedLabels.forEach(function (lines) {
      if (lines.length > maxLines) {
        maxLines = lines.length;
      }
      lines.forEach(function (line) {
        if (line.length > maxLineChars) {
          maxLineChars = line.length;
        }
      });
    });

    var slotMin = Math.max(148, Math.ceil(maxLineChars * 6.8) + 26);
    var padL = 74;
    var padR = Math.max(70, Math.ceil(maxLineChars * 3.6) + 18);
    var padT = 26;
    var padB = 26 + maxLines * 14;
    var W = Math.max(560, config.items.length * slotMin + padL + padR);
    var H = 216 + padB;
    var plotW = W - padL - padR;
    var plotH = H - padT - padB;

    var svg = svgEl("svg", {
      "class": "svg-chart",
      "viewBox": "0 0 " + W + " " + H,
      "role": "img",
      "aria-label": config.title
    });

    for (var g = 0; g <= 4; g++) {
      var frac = g / 4;
      var gy = padT + plotH - frac * plotH;
      svg.appendChild(svgEl("line", {
        "x1": padL, "y1": gy.toFixed(1),
        "x2": W - padR, "y2": gy.toFixed(1),
        "class": g === 0 ? "svg-axis-line" : "svg-grid-line"
      }));
      svg.appendChild(svgEl("text", {
        "x": padL - 8,
        "y": (gy + 4).toFixed(1),
        "text-anchor": "end",
        "class": "svg-axis-label"
      }, config.valueFormatter(maxVal * frac)));
    }

    var slotW = plotW / config.items.length;
    var barW = Math.min(54, Math.max(24, slotW * 0.45));

    config.items.forEach(function (it, idx) {
      var cx = padL + slotW * (idx + 0.5);
      var bx = cx - barW / 2;
      var curY = padT + plotH;
      var total = 0;

      (it.segments || []).forEach(function (seg) {
        var val = Math.max(0, numOrZero(seg.value));
        total += val;
        var sh = (val / maxVal) * plotH;
        if (sh <= 0) {
          return;
        }
        curY -= sh;
        var rect = svgEl("rect", {
          "x": bx.toFixed(1),
          "y": curY.toFixed(1),
          "width": barW.toFixed(1),
          "height": Math.max(1.5, sh).toFixed(1),
          "fill": seg.color,
          "rx": "2",
          "class": "svg-bar"
        });
        bindTooltip(rect, it.label + " · " + seg.key, [
          "Role: " + config.valueFormatter(val),
          "Phase Total: " + config.valueFormatter(it.totalDisplay || total)
        ]);
        svg.appendChild(rect);
      });

      svg.appendChild(svgEl("text", {
        "x": cx.toFixed(1),
        "y": Math.max(14, curY - 6).toFixed(1),
        "text-anchor": "middle",
        "class": "svg-value-label"
      }, config.valueFormatter(it.totalDisplay || total)));

      if (isFiniteNum(it.markerValue) && it.markerValue > 0) {
        var my = padT + plotH - (it.markerValue / maxVal) * plotH;
        var marker = svgEl("polygon", {
          "points": cx.toFixed(1) + "," + (my - 5).toFixed(1) + " " + (cx + 5).toFixed(1) + "," + my.toFixed(1) + " " + cx.toFixed(1) + "," + (my + 5).toFixed(1) + " " + (cx - 5).toFixed(1) + "," + my.toFixed(1),
          "fill": "#f59e0b",
          "stroke": "#78350f",
          "stroke-width": "1",
          "class": "svg-point"
        });
        bindTooltip(marker, it.label + " · EarlyPeak", [
          "EarlyPeak: " + config.valueFormatter(it.markerValue),
          "Steady Median: " + config.valueFormatter(it.totalDisplay || total)
        ]);
        svg.appendChild(marker);
      }

      appendWrappedAxisLabel(svg, cx, padT + plotH + 18, wrappedLabels[idx]);
    });

    var wrap = el("div", "svg-wrap");
    wrap.appendChild(svg);
    panel.appendChild(wrap);
    container.appendChild(panel);
  }

  function renderGroupedBarChart(container, config) {
    /* config: { title, note, categories: [string], series: [{ name, color, values: [number|null] }], valueFormatter, diverging } */
    var panel = el("div", "chart-panel");
    var head = el("div", "chart-header");
    head.appendChild(el("h3", "chart-title", config.title));
    if (config.note) {
      head.appendChild(el("span", "chart-note", config.note));
    }
    panel.appendChild(head);

    if (!config.categories || config.categories.length === 0 || !config.series || config.series.length === 0) {
      panel.appendChild(el("div", "empty-state", "No matching data for grouped chart."));
      container.appendChild(panel);
      return;
    }

    var leg = el("div", "legend-row");
    config.series.forEach(function (s) {
      var li = el("span", "legend-item");
      var sw = el("span", "legend-swatch");
      sw.style.backgroundColor = s.color;
      li.appendChild(sw);
      li.appendChild(document.createTextNode(s.name));
      leg.appendChild(li);
    });
    panel.appendChild(leg);

    var minVal = 0;
    var maxVal = 0;
    config.series.forEach(function (s) {
      (s.values || []).forEach(function (v) {
        if (!isFiniteNum(v)) {
          return;
        }
        if (v < minVal) {
          minVal = v;
        }
        if (v > maxVal) {
          maxVal = v;
        }
      });
    });
    if (!config.diverging) {
      minVal = 0;
    }
    if (maxVal === minVal) {
      maxVal = minVal + 1;
    }
    var range = maxVal - minVal;

    var wrappedCats = config.categories.map(function (c) {
      return wrapAxisLines(c, 18);
    });
    var maxLines = 1;
    var maxLineChars = 8;
    wrappedCats.forEach(function (lines) {
      if (lines.length > maxLines) {
        maxLines = lines.length;
      }
      lines.forEach(function (line) {
        if (line.length > maxLineChars) {
          maxLineChars = line.length;
        }
      });
    });

    var slotMin = Math.max(126, config.series.length * 26 + 38, Math.ceil(maxLineChars * 6.8) + 26);
    var padL = 78;
    var padR = Math.max(74, Math.ceil(maxLineChars * 3.6) + 18);
    var padT = 22;
    var padB = 26 + maxLines * 14;
    var W = Math.max(560, config.categories.length * slotMin + padL + padR);
    var H = 216 + padB;
    var plotW = W - padL - padR;
    var plotH = H - padT - padB;

    var yFor = function (val) {
      return padT + plotH - ((val - minVal) / range) * plotH;
    };
    var zeroY = yFor(0);

    var svg = svgEl("svg", {
      "class": "svg-chart",
      "viewBox": "0 0 " + W + " " + H,
      "role": "img",
      "aria-label": config.title
    });

    for (var g = 0; g <= 4; g++) {
      var vTick = minVal + (range * g) / 4;
      var gy = yFor(vTick);
      svg.appendChild(svgEl("line", {
        "x1": padL, "y1": gy.toFixed(1),
        "x2": W - padR, "y2": gy.toFixed(1),
        "class": Math.abs(vTick) < 1e-9 ? "svg-axis-line" : "svg-grid-line"
      }));
      svg.appendChild(svgEl("text", {
        "x": padL - 8,
        "y": (gy + 4).toFixed(1),
        "text-anchor": "end",
        "class": "svg-axis-label"
      }, config.valueFormatter(vTick)));
    }

    if (minVal < 0 && maxVal > 0) {
      svg.appendChild(svgEl("line", {
        "x1": padL, "y1": zeroY.toFixed(1),
        "x2": W - padR, "y2": zeroY.toFixed(1),
        "class": "svg-axis-line"
      }));
    }

    var groupW = plotW / config.categories.length;
    var innerW = groupW * 0.72;
    var barW = Math.min(34, Math.max(8, innerW / config.series.length - 3));

    config.categories.forEach(function (cat, cIdx) {
      var gx = padL + groupW * cIdx + (groupW - barW * config.series.length - (config.series.length - 1) * 3) / 2;
      config.series.forEach(function (s, sIdx) {
        var val = s.values[cIdx];
        if (!isFiniteNum(val)) {
          return;
        }
        var bx = gx + sIdx * (barW + 3);
        var vy = yFor(val);
        var yTop = Math.min(vy, zeroY);
        var bh = Math.max(1.5, Math.abs(zeroY - vy));
        var rect = svgEl("rect", {
          "x": bx.toFixed(1),
          "y": yTop.toFixed(1),
          "width": barW.toFixed(1),
          "height": bh.toFixed(1),
          "fill": s.color,
          "rx": "2",
          "class": "svg-bar"
        });
        bindTooltip(rect, cat + " · " + s.name, [
          "Value: " + config.valueFormatter(val)
        ]);
        svg.appendChild(rect);
      });

      var labelX = padL + groupW * (cIdx + 0.5);
      appendWrappedAxisLabel(svg, labelX, padT + plotH + 18, wrappedCats[cIdx]);
    });

    var wrap = el("div", "svg-wrap");
    wrap.appendChild(svg);
    panel.appendChild(wrap);
    container.appendChild(panel);
  }

  function renderHorizontalBarChart(container, config) {
    /* config: { title, note, items: [{ label, value, color, extraLines }], valueFormatter } */
    var panel = el("div", "chart-panel");
    var head = el("div", "chart-header");
    head.appendChild(el("h3", "chart-title", config.title));
    if (config.note) {
      head.appendChild(el("span", "chart-note", config.note));
    }
    panel.appendChild(head);

    if (!config.items || config.items.length === 0) {
      panel.appendChild(el("div", "empty-state", "No attribution categories available."));
      container.appendChild(panel);
      return;
    }

    var maxVal = 0;
    config.items.forEach(function (it) {
      var v = Math.max(0, numOrZero(it.value));
      if (v > maxVal) {
        maxVal = v;
      }
    });
    if (maxVal <= 0) {
      maxVal = 1;
    }

    var rowH = 26;
    var padL = 195;
    var padR = 96;
    var padT = 12;
    var padB = 18;
    var W = 720;
    var H = padT + padB + config.items.length * rowH;
    var plotW = W - padL - padR;

    var svg = svgEl("svg", {
      "class": "svg-chart",
      "viewBox": "0 0 " + W + " " + H,
      "role": "img",
      "aria-label": config.title
    });

    config.items.forEach(function (it, idx) {
      var y = padT + idx * rowH;
      var val = Math.max(0, numOrZero(it.value));
      var bw = Math.max(1.5, (val / maxVal) * plotW);
      var lbl = it.label.length > 26 ? it.label.slice(0, 25) + "…" : it.label;

      svg.appendChild(svgEl("text", {
        "x": padL - 10,
        "y": (y + 16).toFixed(1),
        "text-anchor": "end",
        "class": "svg-axis-label"
      }, lbl));

      svg.appendChild(svgEl("rect", {
        "x": padL,
        "y": (y + 4).toFixed(1),
        "width": plotW,
        "height": 16,
        "fill": "var(--grid-line)",
        "opacity": "0.35",
        "rx": "3"
      }));

      var rect = svgEl("rect", {
        "x": padL,
        "y": (y + 4).toFixed(1),
        "width": bw.toFixed(1),
        "height": 16,
        "fill": it.color || "#2563eb",
        "rx": "3",
        "class": "svg-bar"
      });
      var tipLines = ["Primary: " + config.valueFormatter(val)];
      if (Array.isArray(it.extraLines)) {
        tipLines = tipLines.concat(it.extraLines);
      }
      bindTooltip(rect, it.label, tipLines);
      svg.appendChild(rect);

      svg.appendChild(svgEl("text", {
        "x": (padL + bw + 6).toFixed(1),
        "y": (y + 16).toFixed(1),
        "text-anchor": "start",
        "class": "svg-value-label"
      }, config.valueFormatter(val)));
    });

    var wrap = el("div", "svg-wrap");
    wrap.appendChild(svg);
    panel.appendChild(wrap);
    container.appendChild(panel);
  }

  function computeOLS(values) {
    var n = values.length;
    if (n < 2) {
      return null;
    }
    var sumX = 0;
    var sumY = 0;
    var sumXY = 0;
    var sumXX = 0;
    for (var i = 0; i < n; i++) {
      var y = numOrZero(values[i]);
      sumX += i;
      sumY += y;
      sumXY += i * y;
      sumXX += i * i;
    }
    var meanX = sumX / n;
    var meanY = sumY / n;
    var den = sumXX - n * meanX * meanX;
    var slope = den > 0 ? (sumXY - n * meanX * meanY) / den : 0;
    var intercept = meanY - slope * meanX;
    return { slope: slope, intercept: intercept };
  }

  function renderLineSlopeChart(container, config) {
    /* config: { title, note, series: [{ name, color, values: [number], slopeBytesPerRound, passed }] } */
    var panel = el("div", "chart-panel");
    var head = el("div", "chart-header");
    head.appendChild(el("h3", "chart-title", config.title));
    if (config.note) {
      head.appendChild(el("span", "chart-note", config.note));
    }
    panel.appendChild(head);

    var validSeries = (config.series || []).filter(function (s) {
      return Array.isArray(s.values) && s.values.length > 0;
    });
    if (validSeries.length === 0) {
      panel.appendChild(el("div", "empty-state", "No round-by-round leak check samples available."));
      container.appendChild(panel);
      return;
    }

    var leg = el("div", "legend-row");
    validSeries.forEach(function (s) {
      var li = el("span", "legend-item");
      var sw = el("span", "legend-swatch");
      sw.style.backgroundColor = s.color;
      li.appendChild(sw);
      li.appendChild(document.createTextNode(s.name + " (" + formatSlopeMiB(s.slopeBytesPerRound) + ") "));
      var b = el("span", s.passed ? "badge badge-ok" : "badge badge-danger", s.passed ? "PASS" : "FAIL");
      li.appendChild(b);
      leg.appendChild(li);
    });
    panel.appendChild(leg);

    var minY = Infinity;
    var maxY = -Infinity;
    var maxLen = 1;
    validSeries.forEach(function (s) {
      if (s.values.length > maxLen) {
        maxLen = s.values.length;
      }
      s.values.forEach(function (v) {
        if (isFiniteNum(v)) {
          if (v < minY) minY = v;
          if (v > maxY) maxY = v;
        }
      });
    });
    if (!Number.isFinite(minY) || !Number.isFinite(maxY)) {
      minY = 0;
      maxY = 1;
    }
    var padSpan = (maxY - minY) * 0.15;
    if (padSpan <= 0) {
      padSpan = Math.max(1, Math.abs(maxY) * 0.1);
    }
    var loY = Math.max(0, minY - padSpan);
    var hiY = maxY + padSpan;
    var spanY = hiY - loY > 0 ? hiY - loY : 1;

    var W = 720;
    var H = 280;
    var padL = 78;
    var padR = 28;
    var padT = 20;
    var padB = 42;
    var plotW = W - padL - padR;
    var plotH = H - padT - padB;

    var xFor = function (idx) {
      if (maxLen <= 1) {
        return padL + plotW / 2;
      }
      return padL + (idx / (maxLen - 1)) * plotW;
    };
    var yFor = function (val) {
      return padT + plotH - ((val - loY) / spanY) * plotH;
    };

    var svg = svgEl("svg", {
      "class": "svg-chart",
      "viewBox": "0 0 " + W + " " + H,
      "role": "img",
      "aria-label": config.title
    });

    for (var g = 0; g <= 4; g++) {
      var valTick = loY + (spanY * g) / 4;
      var gy = yFor(valTick);
      svg.appendChild(svgEl("line", {
        "x1": padL, "y1": gy.toFixed(1),
        "x2": W - padR, "y2": gy.toFixed(1),
        "class": g === 0 ? "svg-axis-line" : "svg-grid-line"
      }));
      svg.appendChild(svgEl("text", {
        "x": padL - 8,
        "y": (gy + 4).toFixed(1),
        "text-anchor": "end",
        "class": "svg-axis-label"
      }, formatBytes(valTick)));
    }

    for (var r = 0; r < maxLen; r++) {
      svg.appendChild(svgEl("text", {
        "x": xFor(r).toFixed(1),
        "y": (padT + plotH + 20).toFixed(1),
        "text-anchor": "middle",
        "class": "svg-axis-label"
      }, "R" + (r + 1)));
    }

    validSeries.forEach(function (s) {
      var pts = [];
      s.values.forEach(function (v, idx) {
        if (!isFiniteNum(v)) {
          return;
        }
        pts.push(xFor(idx).toFixed(1) + "," + yFor(v).toFixed(1));
      });
      if (pts.length >= 2) {
        svg.appendChild(svgEl("polyline", {
          "fill": "none",
          "stroke": s.color,
          "stroke-width": "2.4",
          "points": pts.join(" ")
        }));
        var ols = computeOLS(s.values);
        if (ols) {
          var y0 = ols.intercept;
          var yN = ols.slope * (s.values.length - 1) + ols.intercept;
          svg.appendChild(svgEl("line", {
            "x1": xFor(0).toFixed(1),
            "y1": yFor(y0).toFixed(1),
            "x2": xFor(s.values.length - 1).toFixed(1),
            "y2": yFor(yN).toFixed(1),
            "stroke": s.color,
            "stroke-width": "1.6",
            "stroke-dasharray": "5 4",
            "opacity": "0.75"
          }));
        }
      }

      s.values.forEach(function (v, idx) {
        if (!isFiniteNum(v)) {
          return;
        }
        var c = svgEl("circle", {
          "cx": xFor(idx).toFixed(1),
          "cy": yFor(v).toFixed(1),
          "r": "4.2",
          "fill": s.color,
          "stroke": "#ffffff",
          "stroke-width": "1.2",
          "class": "svg-point"
        });
        bindTooltip(c, s.name + " · Round " + (idx + 1), [
          "Post-close primary memory: " + formatBytes(v),
          "OLS Slope: " + formatSlopeMiB(s.slopeBytesPerRound)
        ]);
        svg.appendChild(c);
      });
    });

    var wrap = el("div", "svg-wrap");
    wrap.appendChild(svg);
    panel.appendChild(wrap);
    container.appendChild(panel);
  }

  function buildTable(headers, rows) {
    var scroll = el("div", "table-scroll");
    var tbl = el("table");
    var thead = el("thead");
    var htr = el("tr");
    headers.forEach(function (h) {
      var th = el("th", h.num ? "num" : "", h.label);
      htr.appendChild(th);
    });
    thead.appendChild(htr);
    tbl.appendChild(thead);

    var tbody = el("tbody");
    rows.forEach(function (row) {
      var tr = el("tr");
      row.forEach(function (cell, idx) {
        var h = headers[idx] || {};
        var td = el("td", h.num ? "num" : (h.wrap ? "wrap-cell" : ""));
        if (cell && cell.nodeType) {
          td.appendChild(cell);
        } else {
          td.textContent = (cell === undefined || cell === null || cell === "") ? "—" : String(cell);
        }
        tr.appendChild(td);
      });
      tbody.appendChild(tr);
    });
    tbl.appendChild(tbody);
    scroll.appendChild(tbl);
    return scroll;
  }

  function renderOverview() {
    var active = getActiveReports();
    var anomWrap = document.getElementById("anomaly-banner-container");
    var basisWrap = document.getElementById("basis-notice-container");
    var envWrap = document.getElementById("env-cards-container");
    var kpiWrap = document.getElementById("overview-kpi-container");

    anomWrap.replaceChildren();
    basisWrap.replaceChildren();
    envWrap.replaceChildren();
    kpiWrap.replaceChildren();

    var allAnomalies = Array.isArray(payload.anomalies) ? payload.anomalies : [];
    var anomalies = allAnomalies.filter(function (a) {
      if (isFiniteNum(a.report_index) && a.report_index >= 0 && !state.selectedReports.has(a.report_index)) {
        return false;
      }
      if (a.track && state.track !== "all" && a.track !== state.track) {
        return false;
      }
      if (a.dataset && state.dataset !== "all" && a.dataset !== state.dataset) {
        return false;
      }
      return true;
    });
    if (anomalies.length > 0) {
      var b = el("div", "banner banner-danger");
      b.appendChild(el("div", "banner-title", "Anomaly & Regression Signals (" + anomalies.length + ")"));
      var ul = el("ul", "banner-list");
      anomalies.forEach(function (a) {
        var li = el("li");
        li.appendChild(el("span", "badge badge-danger", a.severity || "ALERT"));
        li.appendChild(document.createTextNode(" [" + (a.report_label || "report") + " · " + (a.scenario || "global") + "] " + a.title + " — " + a.detail));
        ul.appendChild(li);
      });
      b.appendChild(ul);
      anomWrap.appendChild(b);
    }

    if (payload.basis_notice) {
      var infoB = el("div", "banner banner-info");
      infoB.appendChild(el("div", "banner-title", payload.cross_platform ? "Cross-Platform Measurement Basis Notice (Side-by-Side Only)" : "Platform Measurement Basis"));
      infoB.appendChild(el("p", "note-line", payload.basis_notice));
      basisWrap.appendChild(infoB);
    }

    active.forEach(function (r) {
      var rep = r.report || {};
      var env = rep.environment || {};
      var clip = rep.clipboard_integrity || {};
      var bins = rep.binaries || {};
      var anyEmu = Object.keys(bins).some(function (k) { return bins[k] && bins[k].emulated; });

      var card = el("div", "subcard");
      var top = el("div", "subcard-title-row");
      top.appendChild(el("h3", "subcard-title", r.label));
      var badges = el("div", "pill-row");
      badges.appendChild(el("span", "badge badge-info", (env.os || "unknown") + "/" + (env.host_arch || "?")));
      badges.appendChild(el("span", "badge badge-neutral", r.primary_metric_name));
      if (anyEmu) {
        badges.appendChild(el("span", "badge badge-danger", "EMULATED BINARY"));
      } else {
        badges.appendChild(el("span", "badge badge-ok", "Native Arch"));
      }
      if (clip.available) {
        badges.appendChild(el("span", clip.unchanged ? "badge badge-ok" : "badge badge-danger", clip.unchanged ? "Clipboard Unchanged (F9)" : "CLIPBOARD MUTATED"));
      } else {
        badges.appendChild(el("span", "badge badge-warn", "Clipboard Check N/A"));
      }
      top.appendChild(badges);
      card.appendChild(top);

      var kv = el("div", "meta-kv-grid");
      var pairs = [
        ["Run ID", rep.run_id || "—"],
        ["OS Version", env.os_version || "—"],
        ["CPU Model", env.cpu_model || "—"],
        ["CPUs / RAM", (env.logical_cpus || 0) + "c / " + formatBytes(env.physical_memory_bytes)],
        ["VM / Hypervisor", env.virtual_machine ? (env.hypervisor || "VM") : (r.platform === "windows" && String(env.cpu_model || "").indexOf("ARMv8") >= 0 ? "Host/VM (see report)" : "Bare Metal / Native")],
        ["Source / Go", (env.git_commit || "—") + (env.git_dirty ? " (dirty)" : "") + " · " + (env.go_version || "—")]
      ];
      pairs.forEach(function (pair) {
        var cell = el("div");
        cell.appendChild(el("span", "meta-kv-label", pair[0]));
        cell.appendChild(el("span", "meta-kv-val", pair[1]));
        kv.appendChild(cell);
      });
      card.appendChild(kv);
      envWrap.appendChild(card);

      renderReportKPIs(kpiWrap, r);
    });
  }

  function renderReportKPIs(kpiWrap, r) {
    var rep = r.report || {};
    var bgStd = findScenarios(r, "bg-idle-normal").find(function (s) { return scenarioDataset(s) === "standard"; }) || findScenarios(r, "bg-idle-normal")[0];
    var bgLight = findScenarios(r, "bg-idle-lightweight")[0];
    var fgVis = findScenarios(r, "fg-visible-idle").find(function (s) { return scenarioDataset(s) === "standard"; }) || findScenarios(r, "fg-visible-idle")[0];
    var closeRec = findScenarios(r, "fg-close-reclaim")[0];
    var qpList = findScenarios(r, "qp-lifecycle");

    var addKPI = function (label, value, sub, badgeNode) {
      var k = el("div", "kpi-card");
      k.appendChild(el("div", "kpi-label", r.label + " · " + label));
      var valRow = el("div", "subcard-title-row");
      valRow.appendChild(el("div", "kpi-value", value));
      if (badgeNode) {
        valRow.appendChild(badgeNode);
      }
      k.appendChild(valRow);
      k.appendChild(el("div", "kpi-sub", sub));
      kpiWrap.appendChild(k);
    };

    if (bgStd && bgStd.phases && bgStd.phases[0]) {
      var p = bgStd.phases[0];
      addKPI(
        "Bg Idle Normal (" + scenarioDataset(bgStd) + ")",
        formatBytes(p.steady_median_bytes),
        r.primary_metric_name + " · EarlyPeak " + formatBytes(p.early_peak_bytes) + " · CPU " + formatPct(p.cpu_percent_mean)
      );
    }

    var netDelta = rep.derived_metrics && rep.derived_metrics.tray_plus_standby_panel_primary_bytes;
    if (isFiniteNum(netDelta) && bgLight) {
      var cpuDelta = rep.derived_metrics.tray_plus_standby_panel_cpu_percent;
      addKPI(
        "Tray + Standby QP Delta",
        formatSignedBytes(netDelta),
        "Normal minus Lightweight (unicd) · CPU " + (isFiniteNum(cpuDelta) ? (cpuDelta >= 0 ? "+" : "") + cpuDelta.toFixed(2) + "%" : "—")
      );
    }

    if (fgVis && fgVis.phases && fgVis.phases[0]) {
      var fp = fgVis.phases[0];
      var visLat = fgVis.latencies && fgVis.latencies.ipc_to_visible_ms;
      addKPI(
        "Fg Visible Idle (" + scenarioDataset(fgVis) + ")",
        formatBytes(fp.steady_median_bytes),
        "EarlyPeak " + formatBytes(fp.early_peak_bytes) + " · ipc_to_visible " + (visLat ? formatMs(visLat.p50_ms) : "—")
      );
    }

    if (closeRec && closeRec.derived_metrics) {
      var ratio = closeRec.derived_metrics.reclaim_ratio_pct;
      var slope = closeRec.leak_check ? closeRec.leak_check.slope_bytes_per_round : null;
      addKPI(
        "Close Reclaim Ratio",
        isFiniteNum(ratio) ? ratio.toFixed(1) + "%" : "—",
        "Residual " + formatSignedBytes(closeRec.derived_metrics.residual_above_baseline_bytes) + " · 5r slope " + formatSlopeMiB(slope),
        closeRec.leak_check ? el("span", closeRec.leak_check.passed ? "badge badge-ok" : "badge badge-danger", closeRec.leak_check.passed ? "PASS" : "FAIL") : null
      );
    }

    qpList.forEach(function (qp) {
      if (!qp.leak_check) {
        return;
      }
      var tr = scenarioTrack(qp, r) || "qp";
      addKPI(
        "QP Warm Slope (" + tr + ")",
        formatSlopeMiB(qp.leak_check.slope_bytes_per_round),
        qp.leak_check.rounds + " rounds · Δ(last-first) " + formatSignedBytes(qp.derived_metrics && qp.derived_metrics.warm_post_close_delta_bytes),
        el("span", qp.leak_check.passed ? "badge badge-ok" : "badge badge-danger", qp.leak_check.passed ? "PASS" : "FAIL")
      );
    });
  }

  function renderBgIdle() {
    var root = document.getElementById("bg-idle-content");
    root.replaceChildren();
    var active = getActiveReports();
    var stackItems = [];
    var rows = [];
    var deltaCards = [];

    active.forEach(function (r, rIdx) {
      var matches = findScenarios(r, "bg-idle-normal").concat(findScenarios(r, "bg-idle-lightweight"));
      matches.forEach(function (sc) {
        var ds = scenarioDataset(sc);
        (sc.phases || []).forEach(function (ph) {
          var byRole = ph.by_role || {};
          var roleNames = Object.keys(byRole).sort();
          var segs = roleNames.map(function (rn) {
            return {
              key: rn,
              value: byRole[rn].primary_memory_bytes ? byRole[rn].primary_memory_bytes.median : 0,
              color: roleColor(rn)
            };
          });
          stackItems.push({
            label: r.label,
            sublabel: (sc.id === "bg-idle-lightweight" ? "lightweight" : "normal") + " (" + ds + ")",
            segments: segs,
            totalDisplay: ph.steady_median_bytes,
            markerValue: ph.early_peak_bytes
          });

          rows.push([
            r.label + " (" + r.primary_metric_name + ")",
            sc.id,
            ds,
            ph.phase,
            formatBytes(ph.steady_median_bytes),
            formatBytes(ph.steady_p95_bytes),
            formatBytes(ph.early_peak_bytes),
            isMetricAvailable(r.platform, "private_commit_bytes", ph.private_commit_bytes) ? formatBytes(ph.private_commit_bytes.median) : "—",
            isMetricAvailable(r.platform, "working_set_bytes", ph.working_set_bytes) ? formatBytes(ph.working_set_bytes.median) : "—",
            formatPct(ph.cpu_percent_mean),
            isMetricAvailable(r.platform, "idle_wakeups_per_sec", ph.idle_wakeups_per_sec) ? numOrZero(ph.idle_wakeups_per_sec).toFixed(2) + "/s" : "—"
          ]);

          roleNames.forEach(function (rn) {
            var rp = byRole[rn];
            rows.push([
              "  ↳ " + rn + " (pid " + rp.pid + ")",
              sc.id,
              ds,
              "role",
              formatBytes(rp.primary_memory_bytes && rp.primary_memory_bytes.median),
              formatBytes(rp.primary_memory_bytes && rp.primary_memory_bytes.p95),
              formatBytes(rp.peak_primary_memory_bytes),
              isMetricAvailable(r.platform, "private_commit_bytes", rp.private_commit_bytes) ? formatBytes(rp.private_commit_bytes.median) : "—",
              isMetricAvailable(r.platform, "working_set_bytes", rp.working_set_bytes) ? formatBytes(rp.working_set_bytes.median) : "—",
              formatPct(rp.cpu_percent_mean),
              isMetricAvailable(r.platform, "idle_wakeups_per_sec", rp.idle_wakeups_per_sec) ? numOrZero(rp.idle_wakeups_per_sec).toFixed(2) + "/s" : "—"
            ]);
          });
        });
      });

      var dm = (r.report && r.report.derived_metrics) || {};
      if (isFiniteNum(dm.tray_plus_standby_panel_primary_bytes)) {
        var c = el("div", "subcard");
        var head = el("div", "subcard-title-row");
        head.appendChild(el("h3", "subcard-title", r.label + " · Tray + Standby Quick Panel Net Overhead"));
        head.appendChild(el("span", "badge badge-info", r.primary_metric_name));
        c.appendChild(head);
        var txt = "Primary Delta: " + formatSignedBytes(dm.tray_plus_standby_panel_primary_bytes) +
          " | CPU Delta: " + (isFiniteNum(dm.tray_plus_standby_panel_cpu_percent) ? (dm.tray_plus_standby_panel_cpu_percent >= 0 ? "+" : "") + dm.tray_plus_standby_panel_cpu_percent.toFixed(2) + "%" : "—");
        if (isFiniteNum(dm.tray_plus_standby_panel_commit_bytes)) {
          txt += " | PrivateCommit Delta: " + formatSignedBytes(dm.tray_plus_standby_panel_commit_bytes);
        }
        c.appendChild(el("p", "note-line", txt));
        deltaCards.push(c);
      }
    });

    if (stackItems.length === 0) {
      root.appendChild(el("div", "empty-state", "No background resident scenarios match current Track/Dataset filters."));
      return;
    }

    renderStackedBarChart(root, {
      title: "Background Steady Memory by Process Role (with EarlyPeak ◆)",
      note: "macOS = ri_phys_footprint | Windows = PrivateWorkingSet (do not subtract cross-OS absolutes)",
      items: stackItems,
      hasMarker: true,
      valueFormatter: formatBytes
    });

    if (deltaCards.length > 0) {
      var dg = el("div", "grid-2");
      deltaCards.forEach(function (c) { dg.appendChild(c); });
      root.appendChild(dg);
    }

    root.appendChild(buildTable([
      { label: "Report / Role" },
      { label: "Scenario" },
      { label: "Dataset" },
      { label: "Phase" },
      { label: "Primary Med", num: true },
      { label: "Primary P95", num: true },
      { label: "Peak / EarlyPeak", num: true },
      { label: "PrivateCommit Med", num: true },
      { label: "WorkingSet Med", num: true },
      { label: "CPU %", num: true },
      { label: "Wakeups/s", num: true }
    ], rows));
  }

  function renderFgInteractive() {
    var root = document.getElementById("fg-interactive-content");
    root.replaceChildren();
    var active = getActiveReports();

    var cats = [];
    var steadyVals = [];
    var p95Vals = [];
    var earlyVals = [];
    var peakVals = [];
    var roleRows = [];
    var probeRows = [];
    var latRows = [];

    active.forEach(function (r) {
      var scs = findScenarios(r, "fg-visible-idle").concat(findScenarios(r, "fg-interactive"));
      scs.forEach(function (sc) {
        var ds = scenarioDataset(sc);
        (sc.phases || []).forEach(function (ph) {
          cats.push(r.label + " · " + ph.phase + " (" + ds + ")");
          steadyVals.push(ph.steady_median_bytes);
          p95Vals.push(ph.steady_p95_bytes);
          earlyVals.push(ph.early_peak_bytes || null);
          peakVals.push(ph.peak_primary_memory_bytes);

          var byRole = ph.by_role || {};
          Object.keys(byRole).sort().forEach(function (rn) {
            var rp = byRole[rn];
            roleRows.push([
              r.label,
              sc.id,
              ds,
              ph.phase,
              rn,
              formatBytes(rp.primary_memory_bytes && rp.primary_memory_bytes.median),
              formatBytes(rp.peak_primary_memory_bytes),
              formatPct(rp.cpu_percent_mean),
              rp.threads_max,
              isMetricAvailable(r.platform, "fds_max", rp.fds_max) ? numOrZero(rp.fds_max) : "—",
              isMetricAvailable(r.platform, "handles_max", rp.handles_max) ? numOrZero(rp.handles_max) : "—",
              isMetricAvailable(r.platform, "gdi_objects_max", rp.gdi_objects_max) ? numOrZero(rp.gdi_objects_max) : "—",
              isMetricAvailable(r.platform, "user_objects_max", rp.user_objects_max) ? numOrZero(rp.user_objects_max) : "—"
            ]);
          });
        });

        if (sc.probe_by_role) {
          Object.keys(sc.probe_by_role).sort().forEach(function (rn) {
            var pr = sc.probe_by_role[rn];
            probeRows.push([
              r.label,
              sc.id,
              rn + (pr.sample_count === 1 ? " (1 sample)" : ""),
              pr.sample_count,
              formatBytes(pr.heap_alloc_max_bytes),
              formatBytes(pr.heap_inuse_last_bytes),
              formatBytes(pr.heap_sys_last_bytes),
              formatBytes(pr.heap_released_last_bytes),
              pr.num_gc_delta,
              pr.goroutines_max,
              (pr.entries_max || pr.details_max || pr.image_owners_max || pr.selected_originals_max)
                ? ("E:" + (pr.entries_max || 0) + " D:" + (pr.details_max || 0) + " Img:" + (pr.image_owners_max || 0) + " Orig:" + (pr.selected_originals_max || 0))
                : "—"
            ]);
          });
        }

        if (sc.latencies) {
          Object.keys(sc.latencies).sort().forEach(function (lk) {
            var lm = sc.latencies[lk];
            latRows.push([
              r.label,
              sc.id + " (" + ds + ")",
              lk,
              lm.count,
              formatMs(lm.p50_ms),
              formatMs(lm.p95_ms),
              formatMs(lm.max_ms)
            ]);
          });
        }
      });
    });

    if (cats.length === 0) {
      root.appendChild(el("div", "empty-state", "No foreground visible/interactive scenarios match current Track/Dataset filters."));
      return;
    }

    renderGroupedBarChart(root, {
      title: "Foreground Visible & Interactive Memory Profile",
      note: "Steady Median / P95 vs EarlyPeak / Peak",
      categories: cats,
      series: [
        { name: "Steady Median", color: "#2563eb", values: steadyVals },
        { name: "Steady P95", color: "#0d9488", values: p95Vals },
        { name: "EarlyPeak", color: "#f59e0b", values: earlyVals },
        { name: "Peak", color: "#7c3aed", values: peakVals }
      ],
      valueFormatter: formatBytes
    });

    root.appendChild(el("h3", "chart-title", "Per-Role Resource Breakdown"));
    root.appendChild(buildTable([
      { label: "Report" },
      { label: "Scenario" },
      { label: "Dataset" },
      { label: "Phase" },
      { label: "Role" },
      { label: "Primary Med", num: true },
      { label: "Peak", num: true },
      { label: "CPU %", num: true },
      { label: "Threads", num: true },
      { label: "FDs", num: true },
      { label: "Handles", num: true },
      { label: "GDI", num: true },
      { label: "USER", num: true }
    ], roleRows));

    if (probeRows.length > 0) {
      root.appendChild(el("h3", "chart-title", "Track B Go Runtime & Desktop UI Cache Attribution (bench_probe)"));
      root.appendChild(buildTable([
        { label: "Report" },
        { label: "Scenario" },
        { label: "Role" },
        { label: "Samples", num: true },
        { label: "HeapAlloc Max", num: true },
        { label: "HeapInuse Last", num: true },
        { label: "HeapSys Last", num: true },
        { label: "HeapReleased", num: true },
        { label: "GC Δ", num: true },
        { label: "Goroutines", num: true },
        { label: "UI Cache Peaks" }
      ], probeRows));
    }

    if (latRows.length > 0) {
      root.appendChild(el("h3", "chart-title", "IPC-to-Ready Latencies"));
      root.appendChild(el("p", "note-line", "Measurement basis: ipc_to_*_ms captures IPC/automation dispatch to readiness only; excludes physical hotkey driver & display compositor first-frame photon latency."));
      root.appendChild(buildTable([
        { label: "Report" },
        { label: "Scenario" },
        { label: "Metric" },
        { label: "Count", num: true },
        { label: "P50", num: true },
        { label: "P95", num: true },
        { label: "Max", num: true }
      ], latRows));
    }
  }

  function renderCloseReclaim() {
    var root = document.getElementById("close-reclaim-content");
    root.replaceChildren();
    var active = getActiveReports();

    var stageNames = [
      "pre-open-bg",
      "open-median (derived)",
      "post-close-bg",
      "pre-hide-visible",
      "gui-hidden-retained",
      "gui-restored"
    ];
    var series = [];
    var leakSeries = [];
    var summaryRows = [];

    active.forEach(function (r, idx) {
      var scs = findScenarios(r, "fg-close-reclaim");
      scs.forEach(function (sc) {
        var phMap = {};
        (sc.phases || []).forEach(function (ph) {
          phMap[ph.phase] = ph.steady_median_bytes;
        });
        var dm = sc.derived_metrics || {};
        var vals = [
          isFiniteNum(phMap["pre-open-bg"]) ? phMap["pre-open-bg"] : (isFiniteNum(dm.baseline_median_bytes) ? dm.baseline_median_bytes : null),
          isFiniteNum(dm.open_median_bytes) ? dm.open_median_bytes : null,
          isFiniteNum(phMap["post-close-bg"]) ? phMap["post-close-bg"] : (isFiniteNum(dm.post_close_median_bytes) ? dm.post_close_median_bytes : null),
          isFiniteNum(phMap["pre-hide-visible"]) ? phMap["pre-hide-visible"] : null,
          isFiniteNum(phMap["gui-hidden-retained"]) ? phMap["gui-hidden-retained"] : null,
          isFiniteNum(phMap["gui-restored"]) ? phMap["gui-restored"] : null
        ];
        series.push({
          name: r.label + " (" + r.primary_metric_name + ")",
          color: reportColor(idx),
          values: vals
        });

        if (sc.leak_check && Array.isArray(sc.leak_check.values_bytes) && sc.leak_check.values_bytes.length > 0) {
          leakSeries.push({
            name: r.label + " post-close",
            color: reportColor(idx),
            values: sc.leak_check.values_bytes,
            slopeBytesPerRound: sc.leak_check.slope_bytes_per_round,
            passed: sc.leak_check.passed
          });
        }

        summaryRows.push([
          r.label,
          formatBytes(dm.baseline_median_bytes),
          formatBytes(dm.open_median_bytes),
          formatSignedBytes(dm.open_spike_bytes),
          formatBytes(dm.post_close_median_bytes),
          formatSignedBytes(dm.reclaimed_bytes),
          isFiniteNum(dm.reclaim_ratio_pct) ? dm.reclaim_ratio_pct.toFixed(1) + "%" : "—",
          formatSignedBytes(dm.residual_above_baseline_bytes),
          formatSignedBytes(dm.hide_retention_delta_bytes),
          sc.leak_check ? formatSlopeMiB(sc.leak_check.slope_bytes_per_round) : "—"
        ]);
      });
    });

    if (series.length === 0) {
      root.appendChild(el("div", "empty-state", "No fg-close-reclaim scenarios match current Track/Dataset filters."));
      return;
    }

    var g2 = el("div", "grid-2");
    renderGroupedBarChart(g2, {
      title: "Close Reclaim vs Hide Retention Stages",
      note: "open-median (derived) sourced from derived_metrics.open_median_bytes",
      categories: stageNames,
      series: series,
      valueFormatter: formatBytes
    });
    renderLineSlopeChart(g2, {
      title: "5-Round Post-Close Background Memory Trend",
      note: "OLS linear regression slope (threshold <= 1.00 MiB/round)",
      series: leakSeries
    });
    root.appendChild(g2);

    root.appendChild(buildTable([
      { label: "Report" },
      { label: "Baseline Med", num: true },
      { label: "5-Round Open Med", num: true },
      { label: "Open Spike", num: true },
      { label: "Post-Close Med", num: true },
      { label: "Reclaimed", num: true },
      { label: "Reclaim Ratio", num: true },
      { label: "Residual", num: true },
      { label: "Hide Δ", num: true },
      { label: "5r Slope", num: true }
    ], summaryRows));
  }

  function renderQpLifecycle() {
    var root = document.getElementById("qp-lifecycle-content");
    root.replaceChildren();
    var active = getActiveReports();

    var leakSeries = [];
    var phaseCategories = ["qp-cold-open", "qp-cold-standby", "qp-warm-open", "qp-warm-closed"];
    var phaseSeries = [];
    var latCategories = [
      "cold_ipc_to_visible_ms",
      "cold_ipc_to_search_ready_ms",
      "ipc_to_visible_ms",
      "ipc_to_search_ready_ms",
      "ipc_to_original_ready_ms"
    ];
    var latSeries = [];
    var latRows = [];
    var driftRows = [];

    var colorCursor = 0;
    active.forEach(function (r) {
      var scs = findScenarios(r, "qp-lifecycle");
      scs.forEach(function (sc) {
        var tr = scenarioTrack(sc, r) || "default";
        var col = reportColor(colorCursor++);
        var tag = r.label + " · " + tr;

        if (sc.leak_check && Array.isArray(sc.leak_check.values_bytes) && sc.leak_check.values_bytes.length > 0) {
          leakSeries.push({
            name: tag,
            color: col,
            values: sc.leak_check.values_bytes,
            slopeBytesPerRound: sc.leak_check.slope_bytes_per_round,
            passed: sc.leak_check.passed
          });
        }

        var phMap = {};
        (sc.phases || []).forEach(function (ph) {
          phMap[ph.phase] = ph.steady_median_bytes;
        });
        phaseSeries.push({
          name: tag,
          color: col,
          values: phaseCategories.map(function (c) { return isFiniteNum(phMap[c]) ? phMap[c] : null; })
        });

        var lats = sc.latencies || {};
        latSeries.push({
          name: tag + " (P95)",
          color: col,
          values: latCategories.map(function (lk) { return lats[lk] ? lats[lk].p95_ms : null; })
        });

        Object.keys(lats).sort().forEach(function (lk) {
          var lm = lats[lk];
          latRows.push([
            r.label,
            tr,
            lk,
            lm.count,
            formatMs(lm.p50_ms),
            formatMs(lm.p95_ms),
            formatMs(lm.max_ms)
          ]);
        });

        var dm = sc.derived_metrics || {};
        var closedPh = (sc.phases || []).find(function (ph) { return ph.phase === "qp-warm-closed"; });
        var qpRole = closedPh && closedPh.by_role && closedPh.by_role["quick-panel"];
        driftRows.push([
          r.label,
          tr,
          sc.leak_check ? formatSlopeMiB(sc.leak_check.slope_bytes_per_round) : "—",
          sc.leak_check ? el("span", sc.leak_check.passed ? "badge badge-ok" : "badge badge-danger", sc.leak_check.passed ? "PASS" : "FAIL") : "—",
          formatBytes(dm.warm_post_close_first_bytes),
          formatBytes(dm.warm_post_close_last_bytes),
          formatSignedBytes(dm.warm_post_close_delta_bytes),
          isMetricAvailable(r.platform, "fds_max", dm.warm_post_close_fds_delta) ? String(numOrZero(dm.warm_post_close_fds_delta)) : "—",
          isMetricAvailable(r.platform, "handles_max", dm.warm_post_close_handles_delta) ? String(numOrZero(dm.warm_post_close_handles_delta)) : "—",
          (qpRole && isMetricAvailable(r.platform, "gdi_objects_max", qpRole.gdi_objects_max)) ? String(numOrZero(qpRole.gdi_objects_max)) : "—",
          (qpRole && isMetricAvailable(r.platform, "user_objects_max", qpRole.user_objects_max)) ? String(numOrZero(qpRole.user_objects_max)) : "—"
        ]);
      });
    });

    if (phaseSeries.length === 0) {
      root.appendChild(el("div", "empty-state", "No qp-lifecycle scenarios match current Track/Dataset filters."));
      return;
    }

    renderLineSlopeChart(root, {
      title: "Warm Quick Panel Post-Close Memory Curve & Linear Regression Slope",
      note: "Multi-round post-close primary memory (dashed line = OLS fit; threshold <= 1.00 MiB/round)",
      series: leakSeries
    });

    var g2 = el("div", "grid-2");
    renderGroupedBarChart(g2, {
      title: "Cold vs Warm Quick Panel Phases",
      note: "Steady median primary memory per phase",
      categories: phaseCategories,
      series: phaseSeries,
      valueFormatter: formatBytes
    });
    renderGroupedBarChart(g2, {
      title: "Quick Panel IPC-to-Ready Latency (P95)",
      note: "Excludes physical hotkey driver & compositor scanout",
      categories: latCategories,
      series: latSeries,
      valueFormatter: formatMs
    });
    root.appendChild(g2);

    root.appendChild(el("h3", "chart-title", "Leak Slope & FD/Handle/GDI/USER Drift"));
    root.appendChild(buildTable([
      { label: "Report" },
      { label: "Track" },
      { label: "Warm Slope", num: true },
      { label: "Check" },
      { label: "R1 Post-Close", num: true },
      { label: "Last Post-Close", num: true },
      { label: "Last - R1", num: true },
      { label: "FDs Δ", num: true },
      { label: "Handles Δ", num: true },
      { label: "QP GDI Max", num: true },
      { label: "QP USER Max", num: true }
    ], driftRows));

    if (latRows.length > 0) {
      root.appendChild(el("h3", "chart-title", "Quick Panel Latency Percentiles"));
      root.appendChild(buildTable([
        { label: "Report" },
        { label: "Track" },
        { label: "Metric" },
        { label: "Count", num: true },
        { label: "P50", num: true },
        { label: "P95", num: true },
        { label: "Max", num: true }
      ], latRows));
    }
  }

  function renderAttribution() {
    var controls = document.getElementById("attribution-controls");
    var root = document.getElementById("attribution-content");
    controls.replaceChildren();
    root.replaceChildren();

    var active = getActiveReports();
    if (active.length === 0) {
      root.appendChild(el("div", "empty-state", "No reports selected."));
      return;
    }

    var repEntry = active.find(function (r) { return r.index === state.attrReportIdx; }) || active[0];
    state.attrReportIdx = repEntry.index;

    var snapshots = [];
    var scs = (repEntry.report && Array.isArray(repEntry.report.scenarios)) ? repEntry.report.scenarios : [];
    scs.forEach(function (sc) {
      if (!scenarioMatches(sc, repEntry)) {
        return;
      }
      (sc.attribution || []).forEach(function (snap, idx) {
        var key = sc.id + "|" + (sc.track || "") + "|" + (sc.dataset || "") + "|" + snap.phase + "|" + idx;
        var label = sc.id + " (" + (sc.track || "") + "/" + (sc.dataset || sc.preset || "") + ") · " + snap.phase;
        snapshots.push({ key: key, label: label, snap: snap });
      });
    });

    if (snapshots.length === 0) {
      root.appendChild(el("div", "empty-state", "No OS attribution snapshots match current filters for " + repEntry.label + "."));
      return;
    }

    var selSnap = snapshots.find(function (s) { return s.key === state.attrScenarioKey; }) || snapshots[0];
    state.attrScenarioKey = selSnap.key;

    var procsMap = selSnap.snap.processes || {};
    var roles = Object.keys(procsMap).map(function (pidKey) {
      return { pid: pidKey, proc: procsMap[pidKey] };
    }).sort(function (a, b) {
      return String(a.proc.role || "").localeCompare(String(b.proc.role || ""));
    });

    if (roles.length === 0) {
      root.appendChild(el("div", "empty-state", "Selected snapshot has no process attribution records."));
      return;
    }

    var selRole = roles.find(function (x) { return x.proc.role === state.attrRole; }) || roles[0];
    state.attrRole = selRole.proc.role;

    /* Controls */
    var repGroup = el("div", "select-group");
    repGroup.appendChild(el("label", "", "Report:"));
    var repSelect = el("select");
    active.forEach(function (r) {
      var opt = el("option", "", r.label + " (" + r.platform + ")");
      opt.value = String(r.index);
      opt.selected = r.index === state.attrReportIdx;
      repSelect.appendChild(opt);
    });
    repSelect.addEventListener("change", function () {
      state.attrReportIdx = Number(repSelect.value);
      state.attrScenarioKey = "";
      renderAttribution();
    });
    repGroup.appendChild(repSelect);
    controls.appendChild(repGroup);

    var snapGroup = el("div", "select-group");
    snapGroup.appendChild(el("label", "", "Scenario / Phase:"));
    var snapSelect = el("select");
    snapshots.forEach(function (s) {
      var opt = el("option", "", s.label);
      opt.value = s.key;
      opt.selected = s.key === state.attrScenarioKey;
      snapSelect.appendChild(opt);
    });
    snapSelect.addEventListener("change", function () {
      state.attrScenarioKey = snapSelect.value;
      renderAttribution();
    });
    snapGroup.appendChild(snapSelect);
    controls.appendChild(snapGroup);

    var roleGroup = el("div", "select-group");
    roleGroup.appendChild(el("label", "", "Process Role:"));
    var roleSelect = el("select");
    roles.forEach(function (x) {
      var opt = el("option", "", x.proc.role + " (PID " + x.proc.pid + ")");
      opt.value = x.proc.role;
      opt.selected = x.proc.role === state.attrRole;
      roleSelect.appendChild(opt);
    });
    roleSelect.addEventListener("change", function () {
      state.attrRole = roleSelect.value;
      renderAttribution();
    });
    roleGroup.appendChild(roleSelect);
    controls.appendChild(roleGroup);

    var pa = selRole.proc;
    var hasCategories = pa.categories && Object.keys(pa.categories).length > 0;
    if (hasCategories || repEntry.platform === "darwin") {
      renderDarwinAttribution(root, pa);
    } else {
      renderWindowsAttribution(root, pa);
    }
  }

  function renderDarwinAttribution(root, pa) {
    var kpis = el("div", "kpi-grid");
    var cards = [
      ["Phys Footprint", formatBytes(pa.phys_footprint_bytes), "Peak: " + formatBytes(pa.phys_footprint_peak_bytes)],
      ["Malloc / VM Allocate", formatBytes(pa.malloc_bytes), "VMAllocate: " + formatBytes(pa.vm_allocate_bytes)],
      ["IOAccelerator / IOSurface", formatBytes(pa.ioaccelerator_bytes), "IOSurface: " + formatBytes(pa.iosurface_bytes)],
      ["Mach Ports", isFiniteNum(pa.mach_ports) ? String(pa.mach_ports) : "—", "Unmapped Owned: " + formatBytes(pa.unmapped_owned_bytes)]
    ];
    cards.forEach(function (c) {
      var k = el("div", "kpi-card");
      k.appendChild(el("div", "kpi-label", c[0]));
      k.appendChild(el("div", "kpi-value", c[1]));
      k.appendChild(el("div", "kpi-sub", c[2]));
      kpis.appendChild(k);
    });
    root.appendChild(kpis);

    var cats = pa.categories || {};
    var sorted = Object.keys(cats).map(function (name) {
      var c = cats[name] || {};
      var primary = numOrZero(c.dirty_bytes) + numOrZero(c.swapped_bytes);
      return { name: name, c: c, primary: primary };
    }).sort(function (a, b) {
      if (b.primary !== a.primary) return b.primary - a.primary;
      var aTotal = numOrZero(a.c.clean_bytes) + numOrZero(a.c.wired_bytes);
      var bTotal = numOrZero(b.c.clean_bytes) + numOrZero(b.c.wired_bytes);
      if (bTotal !== aTotal) return bTotal - aTotal;
      return a.name.localeCompare(b.name);
    });

    var top8 = sorted.slice(0, 8);
    var rest = sorted.slice(8);
    var items = top8.map(function (item, idx) {
      return {
        label: item.name,
        value: item.primary,
        color: SERIES_COLORS[idx % SERIES_COLORS.length],
        extraLines: [
          "Dirty: " + formatBytes(item.c.dirty_bytes) + " | Swapped: " + formatBytes(item.c.swapped_bytes),
          "Clean: " + formatBytes(item.c.clean_bytes) + " | Wired: " + formatBytes(item.c.wired_bytes) + " | Regions: " + (item.c.regions || 0)
        ]
      };
    });
    if (rest.length > 0) {
      var otherSum = 0;
      var otherClean = 0;
      rest.forEach(function (x) {
        otherSum += x.primary;
        otherClean += numOrZero(x.c.clean_bytes);
      });
      items.push({
        label: "Other (" + rest.length + " categories)",
        value: otherSum,
        color: "#64748b",
        extraLines: ["Clean across others: " + formatBytes(otherClean)]
      });
    }

    renderHorizontalBarChart(root, {
      title: "macOS footprint -j Top-8 Categories (Dirty + Swapped) + Other — " + pa.role,
      note: "Ranked by dirty_bytes + swapped_bytes",
      items: items,
      valueFormatter: formatBytes
    });
  }

  function renderWindowsAttribution(root, pa) {
    /* CRITICAL F3: NEVER put ReservedBytes on the same linear axis as Committed bars */
    var kpis = el("div", "kpi-grid");
    var cards = [
      ["Private Committed", formatBytes(pa.private_committed_bytes), "MEM_COMMIT | MEM_PRIVATE"],
      ["Mapped Committed", formatBytes(pa.mapped_committed_bytes), "MEM_COMMIT | MEM_MAPPED"],
      ["Image Committed", formatBytes(pa.image_committed_bytes), "MEM_COMMIT | MEM_IMAGE"],
      ["Reserved Virtual Address Space", formatBytes(pa.reserved_bytes), "Excluded from chart axis (virtual reservation, not physical RAM)"]
    ];
    cards.forEach(function (c) {
      var k = el("div", "kpi-card");
      k.appendChild(el("div", "kpi-label", c[0]));
      k.appendChild(el("div", "kpi-value", c[1]));
      k.appendChild(el("div", "kpi-sub", c[2]));
      kpis.appendChild(k);
    });
    root.appendChild(kpis);

    var noteBox = el("div", "banner banner-warn");
    noteBox.appendChild(el("div", "banner-title", "Windows VirtualQueryEx Attribution Boundary"));
    noteBox.appendChild(el("p", "note-line", (pa.note || "VirtualQueryEx region commits; not GPU VRAM attribution") + " — Virtual ReservedBytes (" + formatBytes(pa.reserved_bytes) + ") is displayed separately above so Go's ~5 GiB virtual address reservation never compresses MiB-scale committed bars."));
    root.appendChild(noteBox);

    renderHorizontalBarChart(root, {
      title: "Windows VirtualQueryEx Committed Regions (Private / Mapped / Image) — " + pa.role,
      note: "Committed bytes only (excludes ReservedBytes)",
      items: [
        { label: "PrivateCommittedBytes", value: pa.private_committed_bytes || 0, color: "#2563eb" },
        { label: "MappedCommittedBytes", value: pa.mapped_committed_bytes || 0, color: "#0d9488" },
        { label: "ImageCommittedBytes", value: pa.image_committed_bytes || 0, color: "#7c3aed" }
      ],
      valueFormatter: formatBytes
    });
  }

  function renderCompare() {
    var sec = document.getElementById("compare");
    var navLink = document.getElementById("nav-compare-link");
    var root = document.getElementById("compare-content");
    root.replaceChildren();

    var cmpWrap = payload.comparison;
    if (!cmpWrap || !cmpWrap.result) {
      sec.hidden = true;
      if (navLink) navLink.hidden = true;
      return;
    }
    sec.hidden = false;
    if (navLink) navLink.hidden = false;

    var res = cmpWrap.result;
    if (cmpWrap.cross_platform_warning) {
      var wb = el("div", "banner banner-warn");
      wb.appendChild(el("div", "banner-title", "Cross-Platform / Cross-Track Comparison Warning"));
      wb.appendChild(el("p", "note-line", cmpWrap.cross_platform_warning));
      root.appendChild(wb);
    }

    var findings = Array.isArray(res.findings) ? res.findings : [];
    var statusBanner = el("div", findings.length > 0 ? "banner banner-danger" : "banner banner-info");
    statusBanner.appendChild(el("div", "banner-title", (findings.length > 0 ? "REGRESSION / BUDGET ALERT (" + findings.length + " finding(s))" : "OK — No regressions or budget violations detected") + " · Baseline: " + res.baseline_run_id + " -> Candidate: " + res.candidate_run_id));
    if (findings.length > 0) {
      var ul = el("ul", "banner-list");
      findings.forEach(function (f) {
        var li = el("li");
        li.appendChild(el("span", f.severity === "BUDGET_EXCEEDED" ? "badge badge-danger" : "badge badge-warn", f.severity));
        li.appendChild(document.createTextNode(" [" + f.scenario + (f.phase ? "/" + f.phase : "") + "] " + f.metric + ": " + f.message + " (threshold " + f.threshold + ")"));
        ul.appendChild(li);
      });
      statusBanner.appendChild(ul);
    }
    root.appendChild(statusBanner);

    var phaseDiffs = Array.isArray(res.phase_diffs) ? res.phase_diffs : [];
    var latDiffs = Array.isArray(res.latency_diffs) ? res.latency_diffs : [];
    var isIncompatible = Boolean(res.incompatible || cmpWrap.cross_platform_warning);

    if (isIncompatible && (phaseDiffs.length > 0 || latDiffs.length > 0)) {
      root.appendChild(el("p", "note-line", "Relative memory/CPU/latency delta charts are suppressed because the compared runs differ in OS, build track, or emulation state; side-by-side values remain listed below with — in delta columns."));
    }

    if (phaseDiffs.length > 0) {
      var comparablePhaseDiffs = phaseDiffs.filter(function (pd) {
        return !pd.base_missing && !pd.cand_missing && !pd.incompatible;
      });
      if (!isIncompatible && comparablePhaseDiffs.length > 0) {
        renderGroupedBarChart(root, {
          title: "Phase Memory & CPU Relative Delta (%)",
          note: "Positive values indicate candidate increase over baseline",
          categories: comparablePhaseDiffs.map(function (pd) { return pd.scenario + "/" + pd.phase; }),
          series: [
            { name: "Mem Delta %", color: "#2563eb", values: comparablePhaseDiffs.map(function (pd) { return pd.mem_delta_pct; }) },
            { name: "CPU Delta %", color: "#d97706", values: comparablePhaseDiffs.map(function (pd) { return pd.cpu_delta_pct; }) }
          ],
          diverging: true,
          valueFormatter: formatPct
        });
      }

      root.appendChild(buildTable([
        { label: "Scenario" },
        { label: "Track" },
        { label: "Dataset" },
        { label: "Phase" },
        { label: "Base Mem", num: true },
        { label: "Cand Mem", num: true },
        { label: "Mem Δ %", num: true },
        { label: "Base CPU", num: true },
        { label: "Cand CPU", num: true },
        { label: "CPU Δ %", num: true }
      ], phaseDiffs.map(function (pd) {
        var hideBase = Boolean(pd.base_missing);
        var hideCand = Boolean(pd.cand_missing);
        var hideDelta = hideBase || hideCand || Boolean(pd.incompatible) || isIncompatible;
        return [
          pd.scenario,
          pd.track || "—",
          pd.dataset || "—",
          pd.phase,
          hideBase ? "—" : formatBytes(pd.base_mem_median),
          hideCand ? "—" : formatBytes(pd.cand_mem_median),
          hideDelta ? "—" : ((pd.mem_delta_pct >= 0 ? "+" : "") + numOrZero(pd.mem_delta_pct).toFixed(1) + "%"),
          hideBase ? "—" : formatPct(pd.base_cpu_percent),
          hideCand ? "—" : formatPct(pd.cand_cpu_percent),
          hideDelta ? "—" : ((pd.cpu_delta_pct >= 0 ? "+" : "") + numOrZero(pd.cpu_delta_pct).toFixed(1) + "%")
        ];
      })));
    }

    if (latDiffs.length > 0) {
      var comparableLatDiffs = latDiffs.filter(function (ld) {
        return !ld.base_missing && !ld.cand_missing && !ld.incompatible;
      });
      if (!isIncompatible && comparableLatDiffs.length > 0) {
        renderGroupedBarChart(root, {
          title: "Latency P95 Delta (ms)",
          note: "Candidate P95 minus Baseline P95",
          categories: comparableLatDiffs.map(function (ld) { return ld.scenario + "/" + ld.metric; }),
          series: [
            { name: "P95 Delta (ms)", color: "#7c3aed", values: comparableLatDiffs.map(function (ld) { return ld.delta_ms; }) }
          ],
          diverging: true,
          valueFormatter: formatMs
        });
      }

      root.appendChild(buildTable([
        { label: "Scenario" },
        { label: "Track" },
        { label: "Dataset" },
        { label: "Metric" },
        { label: "Base P95", num: true },
        { label: "Cand P95", num: true },
        { label: "Delta", num: true }
      ], latDiffs.map(function (ld) {
        var hideBase = Boolean(ld.base_missing);
        var hideCand = Boolean(ld.cand_missing);
        var hideDelta = hideBase || hideCand || Boolean(ld.incompatible) || isIncompatible;
        return [
          ld.scenario,
          ld.track || "—",
          ld.dataset || "—",
          ld.metric,
          hideBase ? "—" : formatMs(ld.base_p95_ms),
          hideCand ? "—" : formatMs(ld.cand_p95_ms),
          hideDelta ? "—" : ((ld.delta_ms >= 0 ? "+" : "") + numOrZero(ld.delta_ms).toFixed(1) + " ms")
        ];
      })));
    }
  }

  function renderBoundaryAndBinaries() {
    var root = document.getElementById("boundary-content");
    root.replaceChildren();

    var boundaries = Array.isArray(payload.not_run_boundaries) ? payload.not_run_boundaries : [];
    var bCard = el("div", "banner banner-warn");
    bCard.appendChild(el("div", "banner-title", "NOT_RUN Honest Coverage Boundaries"));
    var ul = el("ul", "banner-list");
    boundaries.forEach(function (item) {
      ul.appendChild(el("li", "", item));
    });
    bCard.appendChild(ul);
    root.appendChild(bCard);

    var det = el("details", "collapsible");
    det.appendChild(el("summary", "", "Binaries Under Test & SHA-256 Fingerprints"));
    var rows = [];
    getActiveReports().forEach(function (r) {
      var bins = (r.report && r.report.binaries) || {};
      Object.keys(bins).sort().forEach(function (name) {
        var bi = bins[name];
        rows.push([
          r.label,
          name,
          bi.host_arch || "—",
          bi.binary_arch || "—",
          bi.emulated ? el("span", "badge badge-danger", "true") : el("span", "badge badge-ok", "false"),
          Array.isArray(bi.build_tags) && bi.build_tags.length > 0 ? bi.build_tags.join(", ") : "production",
          el("span", "mono", bi.sha256 || "—")
        ]);
      });
    });
    if (rows.length > 0) {
      det.appendChild(buildTable([
        { label: "Report" },
        { label: "Binary" },
        { label: "Host Arch" },
        { label: "Binary Arch" },
        { label: "Emulated" },
        { label: "Build Tags" },
        { label: "SHA-256" }
      ], rows));
    } else {
      det.appendChild(el("p", "note-line", "No binary metadata recorded in selected reports."));
    }
    root.appendChild(det);
  }

  function renderAll() {
    renderOverview();
    renderBgIdle();
    renderFgInteractive();
    renderCloseReclaim();
    renderQpLifecycle();
    renderAttribution();
    renderCompare();
    renderBoundaryAndBinaries();
    syncHeaderScrollPadding();
  }

  initToolbar();
  window.addEventListener("resize", syncHeaderScrollPadding);
  renderAll();
})();
