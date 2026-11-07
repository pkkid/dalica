/* Dalíca website mockup: reveals, the tour's filmstrip viewer, feature tabs,
   the download button that names the visitor's system, and click to enlarge.
   No third-party code. */
(function () {
  var doc = document.documentElement;
  doc.classList.add("js");
  var reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var clamp = function (v, a, b) { return Math.max(a, Math.min(b, v)); };

  // Headline words arrive one after another, quickly.
  $$("h1 .w").forEach(function (w, i) { w.style.setProperty("--n", i); });

  // Reveals.
  var io = new IntersectionObserver(function (es) {
    es.forEach(function (e) { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } });
  }, { rootMargin: "0px 0px -8% 0px", threshold: 0.12 });
  $$(".rv, .tree").forEach(function (el) { io.observe(el); });

  // Header: a hairline once the page moves; the nav marks the section in view.
  var top = $(".top");
  var navLinks = $$('.top nav a[href^="#"]').filter(function (a) { return a.getAttribute("href").length > 1 && $(a.getAttribute("href")); });
  function onScroll() {
    top.classList.toggle("stuck", scrollY > 8);
    var cur = null;
    navLinks.forEach(function (a) {
      var r = $(a.getAttribute("href")).getBoundingClientRect();
      if (r.top < innerHeight * 0.4 && r.bottom > innerHeight * 0.4) cur = a;
    });
    navLinks.forEach(function (a) { a.classList.toggle("current", a === cur); if (a === cur) a.setAttribute("aria-current", "true"); else a.removeAttribute("aria-current"); });
  }

  // Download: name the visitor's system and lift its card. Phones and tablets get a note instead.
  var ua = navigator.userAgent;
  var mobile = /Android|iPhone|iPad|iPod/i.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1);
  var os = mobile ? null : /Mac/.test(ua) ? "mac" : /Win/.test(ua) ? "win" : "linux";
  var main = $("#dl-main"), card = os && $('.os[data-os="' + os + '"]');
  if (main && card) {
    card.classList.add("mine");
    main.href = card.querySelector(".button").href;
    main.querySelector("span").textContent = "Download for " + { linux: "Linux", win: "Windows", mac: "macOS" }[os];
  } else if (main && !os) {
    main.querySelector("span").textContent = "Download for your computer";
    $("#phone-note").hidden = false;
  }

  // Lightbox: fitted first; a click on the picture shows it at actual size, to pan around.
  var lb = $("#lightbox"), lbImg = lb.querySelector("img"), lbClose = lb.querySelector(".lb-close"), lbSize = lb.querySelector(".lb-size"), opener = null;
  function setActual(on) { lb.classList.toggle("actual", on); lbSize.textContent = on ? "Fit to window" : "Actual size"; lbSize.setAttribute("aria-pressed", String(on)); }
  function openLightbox(src, alt, from) {
    opener = from || document.activeElement;
    lbImg.src = src; lbImg.alt = alt || ""; setActual(false); lb.hidden = false;
    document.body.style.overflow = "hidden"; lbClose.focus();
  }
  function closeLightbox() {
    if (lb.hidden) return;
    lb.hidden = true; document.body.style.overflow = "";
    if (opener && opener.focus) opener.focus({ preventScroll: true });
  }
  // In actual size, dragging the picture pans it; a click without a drag switches back to fit.
  var drag = null, dragged = false;
  lbImg.addEventListener("pointerdown", function (e) {
    if (!lb.classList.contains("actual") || e.pointerType === "touch") return;
    drag = { x: e.clientX, y: e.clientY, sl: lb.scrollLeft, st: lb.scrollTop }; dragged = false;
    lbImg.setPointerCapture(e.pointerId); e.preventDefault();
  });
  lbImg.addEventListener("pointermove", function (e) {
    if (!drag) return;
    var dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 4) { dragged = true; lb.classList.add("dragging"); }
    lb.scrollLeft = drag.sl - dx; lb.scrollTop = drag.st - dy;
  });
  function endDrag() { drag = null; lb.classList.remove("dragging"); }
  lbImg.addEventListener("pointerup", endDrag); lbImg.addEventListener("pointercancel", endDrag);
  lb.addEventListener("click", function (e) {
    if (e.target === lbImg && dragged) { dragged = false; return; }
    if (e.target === lbImg || e.target === lbSize) { setActual(!lb.classList.contains("actual")); return; }
    if (e.target.closest(".lb-bar") && e.target !== lbClose && !e.target.closest(".lb-close")) return;
    closeLightbox();
  });
  lb.addEventListener("keydown", function (e) {
    if (e.key === "Escape") closeLightbox();
    if (e.key === "Tab") { e.preventDefault(); (document.activeElement === lbClose ? lbSize : lbClose).focus(); }
  });
  $$("[data-zoom]").forEach(function (b) {
    b.addEventListener("click", function () { var im = b.querySelector("img"); openLightbox(im.src, im.alt, b); });
  });
  // On a phone the guide's chapter list starts folded, so the chapter itself comes first.
  var fold = $(".docs-nav-fold");
  if (fold && matchMedia("(max-width: 899px)").matches) fold.open = false;
  // In the guide, "On this page" marks the section being read.
  var tocLinks = $$(".docs-toc a");
  if (tocLinks.length) {
    var heads = tocLinks.map(function (a) { return document.getElementById(a.getAttribute("href").slice(1)); });
    var spy = function () {
      var cur = 0;
      heads.forEach(function (h, i) { if (h && h.getBoundingClientRect().top < innerHeight * 0.3) cur = i; });
      tocLinks.forEach(function (a, i) { a.classList.toggle("on", i === cur); });
    };
    addEventListener("scroll", spy, { passive: true }); spy();
  }
  // In the guide, every figure's picture opens in the lightbox too.
  $$(".doc figure img").forEach(function (im) {
    im.tabIndex = 0; im.setAttribute("role", "button"); im.classList.add("zoomable");
    function go() { openLightbox(im.currentSrc || im.src, im.alt, im); }
    im.addEventListener("click", go);
    im.addEventListener("keydown", function (e) { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); go(); } });
  });

  // The tour: one large screenshot over a filmstrip of the six steps. Click a frame, use the
  // arrows (or the arrow keys on the strip), or press Play to step through like a slideshow.
  // Without script the steps stay a plain list.
  (function tour() {
    var list = $("#steps"), viewer = $("#viewer"); if (!list || !viewer) return;
    var items = $$(".step", list), n = items.length;
    var stage = $("#stage"), strip = $("#strip"), cap = $("#viewer-caption"), play = $("#v-play");
    var imgs = [], tabs = [], cur = -1, timer = null, DWELL = 5200;
    items.forEach(function (li, i) {
      var src = li.querySelector("img"), title = li.querySelector("h3").textContent;
      var im = new Image(); im.src = src.getAttribute("src"); im.alt = src.alt; im.width = 1800; im.height = 1170;
      im.decoding = "async"; if (i > 0) im.loading = "lazy";
      $("#stage-in").appendChild(im); imgs.push(im);
      var b = document.createElement("button"); b.type = "button"; b.setAttribute("role", "tab");
      b.id = "tour-tab-" + i; b.setAttribute("aria-controls", "viewer-caption");
      b.setAttribute("aria-label", "Step " + (i + 1) + ": " + title); b.title = title;
      b.innerHTML = '<img src="img/thumbs/' + li.dataset.thumb + '.jpg" alt="" width="360" height="234"><span class="bar"></span>';
      b.addEventListener("click", function () { stop(); show(i); });
      b.addEventListener("keydown", function (e) {
        var j = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? n - 1 : null;
        if (j === null) return;
        e.preventDefault(); stop(); show((j + n) % n); tabs[cur].focus();
      });
      strip.appendChild(b); tabs.push(b);
    });
    function resetBars() {
      tabs.forEach(function (t) { var bar = t.querySelector(".bar"); bar.style.transition = "none"; bar.style.transform = "scaleX(0)"; });
    }
    function runBar() {
      resetBars();
      var bar = tabs[cur].querySelector(".bar"); void bar.offsetWidth;
      bar.style.transition = "transform " + DWELL + "ms linear"; bar.style.transform = "scaleX(1)";
    }
    function show(i) {
      if (i === cur) return;
      var dir = cur < 0 ? 0 : (i > cur ? 1 : -1);
      imgs.forEach(function (im, k) {
        if (k === i) { im.style.setProperty("--from", dir); void im.offsetWidth; im.classList.add("on"); im.style.setProperty("--from", 0); }
        else im.classList.toggle("on", false);
      });
      tabs.forEach(function (t, k) { var on = k === i; t.setAttribute("aria-selected", String(on)); t.tabIndex = on ? 0 : -1; });
      var fig = items[i].querySelector("figcaption");
      cap.innerHTML = '<p class="counter">Step ' + (i + 1) + " of " + n + "</p>" + fig.innerHTML;
      cap.setAttribute("aria-labelledby", tabs[i].id);
      cap.classList.toggle("enter", false); void cap.offsetWidth; cap.classList.add("enter");
      stage.setAttribute("aria-label", "Enlarge: " + imgs[i].alt);
      cur = i;
      if (timer) runBar();
    }
    function tick() { show((cur + 1) % n); }
    function start() {
      if (timer) return;
      timer = setInterval(tick, DWELL);
      play.setAttribute("aria-pressed", "true"); play.querySelector("span").textContent = "Pause"; runBar();
    }
    function stop() {
      if (!timer) return;
      clearInterval(timer); timer = null;
      play.setAttribute("aria-pressed", "false"); play.querySelector("span").textContent = "Play"; resetBars();
    }
    play.addEventListener("click", function () { if (timer) stop(); else start(); });
    $("#v-prev").addEventListener("click", function () { stop(); show((cur - 1 + n) % n); });
    $("#v-next").addEventListener("click", function () { stop(); show((cur + 1) % n); });
    stage.addEventListener("click", function () { stop(); openLightbox(imgs[cur].src, imgs[cur].alt, stage); });
    // Leaving the section, or the page, pauses a playing tour.
    new IntersectionObserver(function (es) { if (!es[0].isIntersecting) stop(); }, { threshold: 0.15 }).observe(viewer);
    document.addEventListener("visibilitychange", function () { if (document.hidden) stop(); });
    viewer.hidden = false;
    show(0);
  })();

  // Feature tabs, with an ink pill that glides to the chosen tab.
  (function tabs() {
    var wrap = $("#tabs"); if (!wrap) return;
    var tabs = $$('[role="tab"]', wrap), ink = $("#tab-ink");
    function current() { return tabs.filter(function (t) { return t.getAttribute("aria-selected") === "true"; })[0] || tabs[0]; }
    function place(t) {
      ink.style.width = t.offsetWidth + "px";
      ink.style.transform = "translate(" + t.offsetLeft + "px, " + (t.offsetTop - 6) + "px)";
      ink.style.height = t.offsetHeight + "px";
      ink.style.setProperty("--ink-c", getComputedStyle(t).getPropertyValue("--c"));
    }
    function select(t, focus) {
      tabs.forEach(function (o) {
        var sel = o === t; o.setAttribute("aria-selected", String(sel)); o.tabIndex = sel ? 0 : -1;
        var p = document.getElementById(o.getAttribute("aria-controls"));
        if (sel) { p.hidden = false; p.classList.remove("enter"); void p.offsetWidth; p.classList.add("enter"); } else p.hidden = true;
      });
      place(t); if (focus) t.focus();
    }
    tabs.forEach(function (t, i) {
      t.addEventListener("click", function () { select(t); });
      t.addEventListener("keydown", function (e) {
        var j = e.key === "ArrowRight" ? i + 1 : e.key === "ArrowLeft" ? i - 1 : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : null;
        if (j === null) return; e.preventDefault(); select(tabs[(j + tabs.length) % tabs.length], true);
      });
    });
    requestAnimationFrame(function () { place(current()); });
    addEventListener("resize", function () { place(current()); });
    if (document.fonts) document.fonts.ready.then(function () { place(current()); });
  })();

  addEventListener("scroll", onScroll, { passive: true });
  onScroll();
})();
