/* dalica.pkarma.com: the download button picks the visitor's system, the hex field behind the page, and the
   small interactions (count-up numbers, gentle reveals, click to enlarge). No third-party code. */
/* Hex field: a flat-top hex grid drawn faintly where a soft mask allows (a wide glow at the top of
   the page, patches beside sections), dots that walk the grid's edges leaving short trails, and
   the odd cell that warms up and fades. Viewport-sized canvas, page coordinates, one rAF loop. */
(function () {
  var cvs = document.getElementById("hexfield"), ctx = cvs.getContext("2d");
  var reduce = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;
  var S = 30, SQ3 = Math.sqrt(3), HW = 1.5 * S, HH = SQ3 * S;
  var ORANGE = "254,128,25", CREAM = "235,219,178";
  var W = 0, H = 0, regions = [], walkers = [], glows = [], mouse = { x: -1e4, y: -1e4 }, last = 0, nextGlow = 0;
  var DIRS = [[0, 120, 240], [60, 180, 300]].map(function (set) {
    return set.map(function (a) { var r = a * Math.PI / 180; return [Math.cos(r) * S, Math.sin(r) * S]; });
  });

  function measure() {
    var docW = document.documentElement.clientWidth;
    regions = [{ x: docW / 2, y: 40, rx: Math.max(560, docW * 0.66), ry: 660, k: 1 }];
    document.querySelectorAll("[data-hex]").forEach(function (el) {
      var r = el.getBoundingClientRect(), left = el.dataset.hex === "left";
      regions.push({ x: left ? r.left + 40 : r.right - 40, y: r.top + scrollY + Math.min(r.height, 420) * 0.35, rx: 560, ry: 380, k: 1 });
    });
  }
  function mask(x, y) {
    var m = 0;
    for (var i = 0; i < regions.length; i++) {
      var g = regions[i], dx = (x - g.x) / g.rx, dy = (y - g.y) / g.ry, d = dx * dx + dy * dy;
      if (d < 1) { var v = 1 - d; v = g.k * v * v; if (v > m) m = v; }
    }
    return m;
  }
  function cursor(x, y) {
    var dx = x - mouse.x, dy = y - mouse.y, d = Math.sqrt(dx * dx + dy * dy);
    return d < 170 ? Math.pow(1 - d / 170, 2) : 0;
  }
  function resize() {
    var dpr = Math.min(window.devicePixelRatio || 1, 2);
    W = innerWidth; H = innerHeight;
    cvs.width = Math.round(W * dpr); cvs.height = Math.round(H * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    measure();
  }
  function hexCenter(q, r) { return [HW * q, HH * (r + q / 2)]; }
  function spawn(w) {
    // pick a region, a point inside it, snap to a hex and start on its right-hand corner
    for (var tries = 0; tries < 30; tries++) {
      var g = regions[Math.random() < 0.35 ? 0 : 1 + Math.floor(Math.random() * Math.max(1, regions.length - 1))] || regions[0];
      var a = Math.random() * Math.PI * 2, rr = Math.sqrt(Math.random()) * 0.8;
      var px = g.x + Math.cos(a) * g.rx * rr, py = g.y + Math.sin(a) * g.ry * rr;
      var q = Math.round(px / HW), r = Math.round(py / HH - q / 2), c = hexCenter(q, r);
      var vx = c[0] + S, vy = c[1];
      if (mask(vx, vy) > 0.12) {
        w.x = w.fx = vx; w.y = w.fy = vy; w.type = 0; w.t = 1; w.back = -1; w.trail = [];
        w.speed = 1.4 + Math.random() * 1.6; w.color = Math.random() < 0.72 ? ORANGE : CREAM; w.life = 0;
        return;
      }
    }
  }
  function step(w) {
    // choose the next edge: never straight back if there is another way, prefer brighter ground
    var dirs = DIRS[w.type], best = [], total = 0;
    for (var i = 0; i < 3; i++) {
      if (i === w.back) continue;
      var nx = w.x + dirs[i][0], ny = w.y + dirs[i][1], m = mask(nx, ny);
      if (m < 0.04) continue;
      best.push([i, nx, ny, m]); total += m;
    }
    if (!best.length) { spawn(w); return; }
    var pick = Math.random() * total, k = 0;
    while (k < best.length - 1 && pick > best[k][3]) { pick -= best[k][3]; k++; }
    var b = best[k];
    w.trail.push([w.x, w.y]); if (w.trail.length > 3) w.trail.shift();
    w.fx = w.x; w.fy = w.y; w.x = b[1]; w.y = b[2]; w.t = 0;
    // the way back, as an index into the other set: A[i] reverses to B[(i+1)%3], B[i] to A[(i+2)%3]
    w.back = w.type === 0 ? (b[0] + 1) % 3 : (b[0] + 2) % 3; w.type = 1 - w.type;
  }
  function init() {
    walkers = [];
    var n = Math.round(Math.min(56, Math.max(24, W / 28)));
    for (var i = 0; i < n; i++) { var w = {}; spawn(w); walkers.push(w); }
  }
  function drawGrid(y0, y1) {
    var buckets = [];
    var q0 = Math.floor(-S / HW) - 1, q1 = Math.ceil((W + S) / HW) + 1;
    for (var q = q0; q <= q1; q++) {
      var r0 = Math.floor(y0 / HH - q / 2) - 1, r1 = Math.ceil(y1 / HH - q / 2) + 1;
      for (var r = r0; r <= r1; r++) {
        var c = hexCenter(q, r), m = mask(c[0], c[1]) + cursor(c[0], c[1]) * 0.9;
        if (m < 0.03) continue;
        var bi = Math.min(9, Math.floor(m * 10)), b = buckets[bi] || (buckets[bi] = []);
        b.push(c);
      }
    }
    ctx.lineWidth = 1;
    for (var i = 0; i < buckets.length; i++) {
      if (!buckets[i]) continue;
      ctx.strokeStyle = "rgba(" + CREAM + "," + (0.012 + i * 0.0085).toFixed(4) + ")";
      ctx.beginPath();
      buckets[i].forEach(function (c) {
        var x = c[0], y = c[1];
        ctx.moveTo(x - S, y); ctx.lineTo(x - S / 2, y - HH / 2); ctx.lineTo(x + S / 2, y - HH / 2); ctx.lineTo(x + S, y);
      });
      ctx.stroke();
    }
  }
  function drawGlows(now, y0, y1) {
    glows = glows.filter(function (g) { return now - g.t0 < g.dur; });
    glows.forEach(function (g) {
      if (g.y < y0 - S || g.y > y1 + S) return;
      var p = (now - g.t0) / g.dur, a = Math.sin(Math.PI * p) * g.a;
      ctx.fillStyle = "rgba(" + ORANGE + "," + a.toFixed(4) + ")";
      ctx.beginPath();
      for (var k = 0; k < 6; k++) { var an = k * Math.PI / 3; ctx.lineTo(g.x + Math.cos(an) * (S - 1.5), g.y + Math.sin(an) * (S - 1.5)); }
      ctx.closePath(); ctx.fill();
    });
  }
  function addGlow(now) {
    for (var tries = 0; tries < 12; tries++) {
      var x = Math.random() * W, y = scrollY + Math.random() * H, m = mask(x, y);
      if (m < 0.25) continue;
      var q = Math.round(x / HW), r = Math.round(y / HH - q / 2), c = hexCenter(q, r);
      glows.push({ x: c[0], y: c[1], t0: now, dur: 2600 + Math.random() * 2400, a: 0.05 + 0.07 * m });
      return;
    }
  }
  function drawWalkers(y0, y1) {
    walkers.forEach(function (w) {
      var t = w.t, x = w.fx + (w.x - w.fx) * t, y = w.fy + (w.y - w.fy) * t;
      if (y < y0 - 2 * S || y > y1 + 2 * S) return;
      var m = Math.min(1, mask(x, y) * 1.3 + 0.15);
      var pts = w.trail.concat([[w.fx, w.fy], [x, y]]);
      ctx.lineCap = "round";
      for (var i = 1; i < pts.length; i++) {
        var f = i / (pts.length - 1);
        ctx.strokeStyle = "rgba(" + w.color + "," + (0.5 * f * f * m).toFixed(3) + ")";
        ctx.lineWidth = 1.6;
        ctx.beginPath(); ctx.moveTo(pts[i - 1][0], pts[i - 1][1]); ctx.lineTo(pts[i][0], pts[i][1]); ctx.stroke();
      }
      ctx.fillStyle = "rgba(" + w.color + "," + (0.16 * m).toFixed(3) + ")";
      ctx.beginPath(); ctx.arc(x, y, 6, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = "rgba(" + w.color + "," + (0.95 * m).toFixed(3) + ")";
      ctx.beginPath(); ctx.arc(x, y, 2.1, 0, Math.PI * 2); ctx.fill();
    });
  }
  function frame(now) {
    var dt = Math.min(0.05, (now - (last || now)) / 1000); last = now;
    var y0 = scrollY, y1 = scrollY + H;
    if (!reduce) {
      walkers.forEach(function (w) { w.t += dt * w.speed; if (w.t >= 1) step(w); });
      if (now > nextGlow) { addGlow(now); nextGlow = now + 450 + Math.random() * 700; }
    }
    ctx.clearRect(0, 0, W, H);
    ctx.save(); ctx.translate(0, -y0);
    drawGrid(y0, y1); drawGlows(now, y0, y1); drawWalkers(y0, y1);
    ctx.restore();
    if (!reduce) requestAnimationFrame(frame);
  }
  resize(); init();
  addEventListener("resize", function () { resize(); init(); if (reduce) frame(performance.now()); });
  addEventListener("load", function () { measure(); });
  addEventListener("pointermove", function (e) { mouse.x = e.clientX; mouse.y = e.clientY + scrollY; });
  addEventListener("pointerleave", function () { mouse.x = mouse.y = -1e4; });
  if (reduce) { walkers.forEach(function (w) { w.t = 0.5; }); addEventListener("scroll", function () { frame(performance.now()); }, { passive: true }); }
  requestAnimationFrame(frame);
})();

// The release on offer. Bump these two at each release (website/README.md → Releasing): the
// version, and the file sizes in bytes, which the jq one-liner there prints from GitHub.
var RELEASE = {
  version: "0.7.0",
  sizes: {"SHA256SUMS.txt":1605,"dalica-cli-{v}-macos-universal.zip":23385783,"dalica-{v}-linux-aarch64.AppImage":24009224,"dalica-{v}-linux-aarch64.AppImage.zsync":82309,"dalica-{v}-linux-aarch64.deb":26784830,"dalica-{v}-linux-aarch64.flatpak":15730048,"dalica-{v}-linux-aarch64.rpm":26518337,"dalica-{v}-linux-aarch64.tar.gz":25553814,"dalica-{v}-linux-x86_64.AppImage":26618360,"dalica-{v}-linux-x86_64.AppImage.zsync":91225,"dalica-{v}-linux-x86_64.deb":28976006,"dalica-{v}-linux-x86_64.flatpak":18441816,"dalica-{v}-linux-x86_64.rpm":28794158,"dalica-{v}-linux-x86_64.tar.gz":28293409,"dalica-{v}-macos-universal.dmg":30479656,"dalica-{v}-windows-x64-portable.zip":27129136,"dalica-{v}-windows-x64.msi":20873216}
};

// Download links: every [data-file] names its file with {v} for the version. Without this script
// the links fall back to the latest release's page on GitHub, which lists the same files.
(function () {
  var v = RELEASE.version, tag = "v" + v, base = "https://github.com/pkkid/dalica/releases/download/" + tag + "/";
  document.querySelectorAll("[data-v]").forEach(function (el) {
    el.textContent = el.dataset.v === "short" ? " " + v.split(".").slice(0, 2).join(".") : v;
  });
  var notes = document.getElementById("dl-notes"); if (notes) notes.href = "https://github.com/pkkid/dalica/releases/tag/" + tag;
  function fill(a) {
    var key = a.dataset.file, file = key.replace("{v}", v), bytes = RELEASE.sizes[key];
    a.href = base + file;
    a.title = file;
    var name = a.querySelector(".fname"); if (name) name.textContent = file;
    var size = a.querySelector(".size"); if (size && bytes) size.textContent = (bytes / 1e6).toFixed(1) + " MB";
  }
  document.querySelectorAll("[data-file]").forEach(fill);

  // The hero button names the visitor's system; its card is lifted a little.
  var ua = navigator.userAgent, os = /Mac/.test(ua) ? "mac" : /Win/.test(ua) ? "win" : "linux";
  var arm = os === "linux" && /aarch64|arm64/i.test(ua);
  var names = { linux: ["Linux", "AppImage · " + (arm ? "ARM64" : "x86_64")], win: ["Windows", "Installer · x64"], mac: ["macOS", "Disk image · Universal"] };
  document.getElementById("dl-main").firstChild.nodeValue = "Download for " + names[os][0];
  document.getElementById("dl-sub").textContent = names[os][1] + " · " + v;
  var card = document.querySelector('.os[data-os="' + os + '"]'); if (card) card.classList.add("mine");
  if (arm && card) {
    // ARM64 Linux: the AppImage on offer is the aarch64 one, and its list starts open.
    var get = card.querySelector(".get"), det = card.querySelector("details.arm");
    if (get) { get.dataset.file = "dalica-{v}-linux-aarch64.AppImage"; fill(get); }
    if (det) det.open = true;
  }
})();
(function () {
  var reduce = window.matchMedia && matchMedia("(prefers-reduced-motion: reduce)").matches;

  // Gentle reveal: anything below the fold starts a little low and rises into place.
  var risers = [].slice.call(document.querySelectorAll(".rise, .shot, .os, .feat, .releases li"));
  if (!reduce && "IntersectionObserver" in window) {
    risers.forEach(function (el) { if (el.getBoundingClientRect().top > innerHeight) { el.classList.add("rise", "low"); } });
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) { if (e.isIntersecting) { e.target.classList.remove("low"); io.unobserve(e.target); } });
    }, { rootMargin: "0px 0px -8% 0px" });
    risers.forEach(function (el) { io.observe(el); });
  }

  // Numbers count up once when they first come into view; the final value is in the HTML.
  function fmt(v, el) {
    var dec = +(el.dataset.dec || 0), t = v.toFixed(dec);
    return el.dataset.sep ? Number(t).toLocaleString("en-US") : t;
  }
  if (!reduce && "IntersectionObserver" in window) {
    var co = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        co.unobserve(e.target);
        var el = e.target, to = +el.dataset.to, t0 = performance.now(), dur = 1400;
        (function tick(now) {
          var p = Math.min(1, (now - t0) / dur), k = 1 - Math.pow(1 - p, 3);
          el.textContent = fmt(to * k, el);
          if (p < 1) requestAnimationFrame(tick);
        })(t0);
      });
    }, { threshold: .6 });
    document.querySelectorAll(".count").forEach(function (el) { co.observe(el); });
  }

  // Click a screenshot to see it full size.
  var lb = document.getElementById("lightbox"), lbImg = lb.querySelector("img"), last = null;
  function close() { lb.hidden = true; lbImg.removeAttribute("src"); if (last) last.focus(); }
  document.querySelectorAll(".frame img").forEach(function (img) {
    img.tabIndex = 0;
    img.setAttribute("role", "button");
    function open() { last = img; lbImg.src = img.currentSrc || img.src; lbImg.alt = img.alt; lb.hidden = false; document.getElementById("lb-close").focus(); }
    img.addEventListener("click", open);
    img.addEventListener("keydown", function (e) { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(); } });
  });
  lb.addEventListener("click", close);
  addEventListener("keydown", function (e) { if (e.key === "Escape" && !lb.hidden) close(); });
})();
