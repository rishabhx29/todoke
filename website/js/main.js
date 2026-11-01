/* Todoke — todoke.tech
   Progressive enhancement only: the page is complete without it.

   Motion stack (all vendored locally):
   - Lenis      smooth scrolling, wired into GSAP's ticker
   - GSAP       scroll choreography (batched luxe-eased reveals, pinned
                how-it-works cascade, scrubbed benchmark fills, motif parallax)
   - anime.js   micro-interactions where a timeline is overkill (FAQ accordion,
                OS chip pop)
   Everything degrades: no JS → static page; reduced motion → IO reveals only,
   no Lenis/pinning/scrubs; the Three.js hero lives in js/hero3d.js.

   ── EDIT HERE ON LAUNCH DAY ─────────────────────────────────────────
   1. Set LAUNCHED to true.
   2. Optional: put the real release asset filenames in ASSETS for
      direct download links. Left as-is, buttons land on the
      latest-release page instead.
   Every download button in #download picks these up automatically.
   Pre-launch, the buttons are placeholder anchors (no href) and the
   tiles say "coming with v1" — nothing links to a 404.
   ─────────────────────────────────────────────────────────────────── */
var RELEASES = "https://github.com/todokeapp/todoke/releases";
var LAUNCHED = false;
var ASSETS = {
  windows: "Todoke_<version>_x64-setup.exe",
  macos:   "Todoke_<version>_universal.dmg",
  linux:   "Todoke_<version>_amd64.AppImage",
};

(function () {
  "use strict";

  document.documentElement.classList.add("js");

  var yearEl = document.getElementById("year");
  if (yearEl) yearEl.textContent = String(new Date().getFullYear());

  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var hasGsap = typeof gsap !== "undefined" && typeof ScrollTrigger !== "undefined";

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
        if (!reduce && typeof anime !== "undefined") {
          anime({
            targets: chip,
            scale: [0.6, 1],
            opacity: [0, 1],
            duration: 520,
            easing: "cubicBezier(0.25, 0.74, 0.22, 0.99)",
          });
        }
      }
    }
  }

  /* Magnetic hover on primary buttons — small pull toward the cursor,
     skipped for reduced motion and touch-only devices. */
  if (!reduce && window.matchMedia("(hover: hover)").matches) {
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

  /* ------------------------------------------------------------------
     Reveal + choreography
     ------------------------------------------------------------------ */
  var revealEls = document.querySelectorAll("[data-reveal]");

  if (reduce || !hasGsap) {
    /* Fallback path — IntersectionObserver one-shot reveals; the page
       is fully visible without JS (CSS gates on .js, not .gsap). */
    if ("IntersectionObserver" in window) {
      var io = new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-in");
            io.unobserve(entry.target);
          }
        });
      }, { threshold: 0.15, rootMargin: "0px 0px -8% 0px" });
      revealEls.forEach(function (el) { io.observe(el); });
    } else {
      revealEls.forEach(function (el) { el.classList.add("is-in"); });
    }
    return;
  }

  /* ---------------- GSAP path ---------------- */
  document.documentElement.classList.add("gsap");
  gsap.registerPlugin(ScrollTrigger);
  var EASE = "power3.out";
  if (typeof CustomEase !== "undefined") {
    CustomEase.create("luxe", "0.25, 0.74, 0.22, 0.99");
    EASE = "luxe";
  }

  /* Lenis smooth scroll, wired to GSAP's ticker (single rAF loop). */
  var lenis = null;
  if (typeof Lenis !== "undefined") {
    lenis = new Lenis({
      duration: 1.1,
      easing: function (t) { return Math.min(1, 1.001 - Math.pow(2, -10 * t)); },
    });
    lenis.on("scroll", ScrollTrigger.update);
    gsap.ticker.add(function (t) { lenis.raf(t * 1000); });
    gsap.ticker.lagSmoothing(0);

    /* anchor links: let Lenis carry the trip, keep keyboard focus right */
    document.querySelectorAll('a[href^="#"]').forEach(function (a) {
      a.addEventListener("click", function (e) {
        var id = a.getAttribute("href").slice(1);
        var target = id ? document.getElementById(id) : null;
        if (!target) return;
        e.preventDefault();
        lenis.scrollTo(target, { offset: -84, duration: 1.2 });
        target.setAttribute("tabindex", "-1");
        target.focus({ preventScroll: true });
      });
    });
  }

  /* Batched reveals — luxe ease, ~1.4 s, 90 ms stagger between siblings. */
  gsap.set("[data-reveal]", { opacity: 0, y: 44 });
  ScrollTrigger.batch("[data-reveal]", {
    start: "top 88%",
    once: true,
    onEnter: function (batch) {
      gsap.to(batch, {
        opacity: 1, y: 0,
        duration: 1.4, ease: EASE, stagger: 0.09, overwrite: true,
      });
    },
  });

  /* Feature motifs drift against the scroll — depth without noise. */
  gsap.utils.toArray(".feature__motif").forEach(function (motif) {
    gsap.fromTo(motif, { y: 34 }, {
      y: -34, ease: "none",
      scrollTrigger: { trigger: motif, start: "top bottom", end: "bottom top", scrub: 0.6 },
    });
  });

  /* How-it-works: pinned section, the three steps cascade in on scrub.
     Steps are pulled out of the batch reveal first so nothing drives them
     twice; below 900 px (or any reduced-motion visitor) they fall back to
     the normal batched reveal. */
  var steps = gsap.utils.toArray(".step");
  steps.forEach(function (el) { el.removeAttribute("data-reveal"); });

  var mm = gsap.matchMedia();
  mm.add("(min-width: 900px) and (prefers-reduced-motion: no-preference)", function () {
    var tl = gsap.timeline({
      scrollTrigger: {
        trigger: "#how",
        start: "top top+=72",
        end: "+=60%",
        pin: true,
        scrub: 0.6,
        anticipatePin: 1,
      },
    });
    tl.from(steps, { opacity: 0, y: 64, stagger: 0.3, duration: 0.65, ease: EASE })
      .to({}, { duration: 0.35 }); // hold the full stack before unpinning
  });

  /* Benchmarks: rows arrive staggered; the placeholder slots fill with a
     scrubbed sweep — "measured soon", visibly on its way. */
  gsap.utils.toArray(".bench__row").forEach(function (row, i) {
    gsap.from(row, {
      opacity: 0, x: -28, duration: 1.1, ease: EASE, delay: i * 0.08,
      scrollTrigger: { trigger: row, start: "top 90%", once: true },
    });
  });
  gsap.utils.toArray(".bench__slot").forEach(function (slot) {
    gsap.fromTo(slot, { scaleX: 0, transformOrigin: "left center" }, {
      scaleX: 1, ease: "none",
      scrollTrigger: { trigger: slot, start: "top 95%", end: "top 45%", scrub: 0.5 },
    });
  });

  /* ---------------- anime.js micros ---------------- */
  if (typeof anime !== "undefined") {
    /* FAQ accordion — animated open/close, still native details/summary
       underneath, so keyboard and no-JS behavior are untouched. */
    document.querySelectorAll(".faq__item").forEach(function (item) {
      var sum = item.querySelector("summary");
      var body = item.querySelector(".faq__a");
      if (!sum || !body) return;
      body.style.overflow = "hidden";
      sum.setAttribute("aria-expanded", item.open ? "true" : "false");
      sum.addEventListener("click", function (e) {
        e.preventDefault();
        if (item.dataset.animating) return;
        item.dataset.animating = "1";
        var done = function () {
          delete item.dataset.animating;
          body.style.height = "";
          body.style.opacity = "";
        };
        if (item.open) {
          sum.setAttribute("aria-expanded", "false");
          anime({
            targets: body,
            height: [body.scrollHeight, 0],
            opacity: [1, 0],
            duration: 340,
            easing: "cubicBezier(0.25, 0.74, 0.22, 0.99)",
            complete: function () { item.open = false; done(); },
          });
        } else {
          item.open = true;
          sum.setAttribute("aria-expanded", "true");
          anime({
            targets: body,
            height: [0, body.scrollHeight],
            opacity: [0, 1],
            duration: 460,
            easing: "cubicBezier(0.25, 0.74, 0.22, 0.99)",
            complete: done,
          });
        }
      });
    });
  }
})();
