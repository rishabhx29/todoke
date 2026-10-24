/* Dasho — dasho.tech
   Progressive enhancement only: the page is complete without it.

   ── EDIT HERE ON LAUNCH DAY ─────────────────────────────────────────
   1. Set LAUNCHED to true.
   2. Optional: put the real release asset filenames in ASSETS for
      direct download links. Left as-is, buttons land on the
      latest-release page instead.
   Every download button in #download picks these up automatically.
   Pre-launch, the buttons are placeholder anchors (no href) and the
   tiles say "coming with v1" — nothing links to a 404.
   ─────────────────────────────────────────────────────────────────── */
var RELEASES = "https://github.com/dashoapp/dasho/releases";
var LAUNCHED = false;
var ASSETS = {
  windows: "Dasho_<version>_x64-setup.exe",
  macos:   "Dasho_<version>_universal.dmg",
  linux:   "Dasho_<version>_amd64.AppImage",
};

(function () {
  "use strict";

  document.documentElement.classList.add("js");

  var yearEl = document.getElementById("year");
  if (yearEl) yearEl.textContent = String(new Date().getFullYear());

  /* Scroll reveals — single-shot; content is fully visible without JS.
     prefers-reduced-motion is handled in CSS (transitions collapse to 0). */
  var revealed = document.querySelectorAll("[data-reveal]");
  if ("IntersectionObserver" in window) {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-in");
          io.unobserve(entry.target);
        }
      });
    }, { threshold: 0.15, rootMargin: "0px 0px -8% 0px" });
    revealed.forEach(function (el) { io.observe(el); });
  } else {
    revealed.forEach(function (el) { el.classList.add("is-in"); });
  }

  /* Magnetic hover on primary buttons — small pull toward the cursor,
     skipped for reduced motion and touch-only devices. */
  var noMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  if (!noMotion.matches && window.matchMedia("(hover: hover)").matches) {
    document.querySelectorAll(".btn--primary").forEach(function (btn) {
      btn.addEventListener("pointermove", function (e) {
        var r = btn.getBoundingClientRect();
        btn.style.setProperty("--mx", ((e.clientX - r.left - r.width / 2) * 0.18).toFixed(1) + "px");
        btn.style.setProperty("--my", ((e.clientY - r.top - r.height / 2) * 0.3).toFixed(1) + "px");
      });
      btn.addEventListener("pointerleave", function () {
        btn.style.setProperty("--mx", "0px");
        btn.style.setProperty("--my", "0px");
      });
    });
  }

  /* Downloads — hrefs come from the constants above, one config place. */
  var grid = document.getElementById("dl-grid");
  if (grid) {
    if (LAUNCHED) {
      document.querySelectorAll("[data-dl]").forEach(function (a) {
        var asset = ASSETS[a.getAttribute("data-dl")];
        // direct asset link only when a real filename is filled in;
        // otherwise land on the latest-release page where GitHub lists them
        if (asset && asset.indexOf("<version>") === -1) {
          a.setAttribute("href", RELEASES + "/latest/download/" + asset);
        } else {
          a.setAttribute("href", RELEASES + "/latest");
        }
      });
      grid.classList.add("dl--live");
      grid.querySelectorAll(".dl__tag").forEach(function (t) { t.hidden = true; });
    } else {
      grid.classList.add("dl--soon");
    }

    /* OS detection — highlight the visitor's platform, keep all three visible. */
    var os = null;
    var ua = navigator.userAgent;
    if (/Windows/i.test(ua)) os = "windows";
    else if (/Mac|iPhone|iPad/i.test(ua)) os = "macos";
    else if (/Linux|X11|Android/i.test(ua)) os = "linux";
    if (os) {
      var match = grid.querySelector('[data-os="' + os + '"]');
      if (match) {
        match.classList.add("dl__os--yours");
        var chip = document.createElement("span");
        chip.className = "dl__yours";
        chip.textContent = "your platform";
        match.prepend(chip);
      }
    }
  }
})();
