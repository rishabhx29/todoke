/* Todoke — zen edition choreography.
   Stack: Lenis (smooth scroll), GSAP/ScrollTrigger (scrubs), anime.js
   (counters), Three.js hero (lazy, via hero3d.js). Everything degrades:
   missing lib → feature silently skipped; reduced motion / automation →
   no smooth scroll, no hero, everything already visible. */

document.documentElement.classList.add("js");

const REDUCED = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const FINE = window.matchMedia("(hover: hover) and (pointer: fine)").matches;
/* Screenshot/test harnesses run under automation; ?motion=1 opts back in so
   the real choreography stays probe-able. Everyone else gets the full ride. */
const AUTOMATED = (navigator.webdriver || /HeadlessChrome/i.test(navigator.userAgent))
  && !new URLSearchParams(location.search).has("motion");
const pad2 = (n) => String(n).padStart(2, "0");

/* ---------------------------------------------------------------- nav */
const head = document.querySelector(".site-head");
const onScroll = () => head?.classList.toggle("is-scrolled", window.scrollY > 8);
onScroll();
window.addEventListener("scroll", onScroll, { passive: true });

document.getElementById("year")?.replaceChildren(document.createTextNode(String(new Date().getFullYear())));

/* ---------------------------------------------------------------- reveals */
const revealEls = [...document.querySelectorAll("[data-reveal]")];
if ("IntersectionObserver" in window && revealEls.length) {
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        e.target.classList.add("is-in");
        io.unobserve(e.target);
      }
    }
  }, { rootMargin: "0px 0px -8% 0px", threshold: 0.05 });
  revealEls.forEach((el) => io.observe(el));
} else {
  revealEls.forEach((el) => el.classList.add("is-in"));
}

/* ------------------------------------------------- feature ⇄ view swap */
const featItems = [...document.querySelectorAll(".featitem")];
const featViews = [...document.querySelectorAll(".featview__stack img")];
const featIdx = document.querySelector(".featview__idx");
function setFeature(i) {
  featItems.forEach((el, j) => el.classList.toggle("is-active", i === j));
  featViews.forEach((el, j) => el.classList.toggle("is-active", i === j));
  if (featIdx) featIdx.textContent = pad2(i + 1);
}
if (featItems.length && featViews.length && "IntersectionObserver" in window) {
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) setFeature(Number(e.target.dataset.feat));
    }
  }, { rootMargin: "-42% 0px -42% 0px" });
  featItems.forEach((el) => io.observe(el));
}

/* --------------------------------------------------- how step ⇄ number */
const howSteps = [...document.querySelectorAll(".howstep")];
const howNum = document.querySelector(".howview__num");
function setStep(i) {
  if (!howNum) return;
  howNum.style.setProperty("--how-i", String(i));
  howNum.querySelectorAll(".howview__fig").forEach((el, j) => el.classList.toggle("is-active", i === j));
}
if (howSteps.length && howNum && "IntersectionObserver" in window) {
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) setStep(Number(e.target.dataset.step));
    }
  }, { rootMargin: "-42% 0px -42% 0px" });
  howSteps.forEach((el) => io.observe(el));
}

/* ---------------------------------------------------------- counters */
const counters = [...document.querySelectorAll(".stat__num")];
function runCounter(el) {
  const finish = () => {
    el.textContent = Number(el.dataset.count).toLocaleString("en-US") + (el.dataset.suffix || "");
  };
  if (!window.anime || REDUCED) { finish(); return; } // no engine / no motion: land on the number
  const target = Number(el.dataset.count) || 0;
  const suffix = el.dataset.suffix || "";
  const state = { v: 0 };
  window.anime({
    targets: state,
    v: target,
    round: 1,
    duration: 1500,
    easing: "easeOutExpo",
    update: () => { el.textContent = state.v.toLocaleString("en-US") + suffix; },
  });
}
const libsReady = initLibs(); // resolves once gsap/anime are in (or have failed)
if (counters.length && "IntersectionObserver" in window) {
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        libsReady.then(() => runCounter(e.target));
        io.unobserve(e.target);
      }
    }
  }, { threshold: 0.5 });
  counters.forEach((el) => io.observe(el));
}

/* UMD vendor files run (and set their globals) when dynamically imported;
   loading them here keeps 130 KB of animation libs off the critical path. */
async function initLibs() {
  if (REDUCED || AUTOMATED) return;
  try {
    await Promise.all([
      import("../vendor/gsap/gsap.min.js"),
      import("../vendor/gsap/ScrollTrigger.min.js"),
      import("../vendor/gsap/CustomEase.min.js"),
      import("../vendor/anime/anime.min.js"),
    ]);
  } catch (e) {
    console.warn("animation libs unavailable, static fallbacks stay", e);
  }
}

/* -------------------------------------------------- GSAP scrubs + lenis */
async function initMotion() {
  await libsReady; // gsap/anime are in (or failed) — branches below are final
  if (!REDUCED && !AUTOMATED) {
    try {
      const { default: Lenis } = await import("../vendor/lenis/lenis.mjs");
      const lenis = new Lenis({ lerp: 0.1 });
      let rafRunning = true;
      const raf = (t) => {
        if (!rafRunning || !lenis) return;
        lenis.raf(t);
        requestAnimationFrame(raf);
      };
      requestAnimationFrame(raf);
      // snap tool / test harness contract: stop the raf loop so scrollY stays put
      window.__todokeLenisDestroy = () => { rafRunning = false; try { lenis.destroy(); } catch { /* already gone */ } };
      if (window.gsap && window.ScrollTrigger) {
        lenis.on("scroll", window.ScrollTrigger.update);
      }
      window.__todokeLenis = lenis;
    } catch (e) {
      console.warn("lenis unavailable, native scroll stays", e);
    }
  }

  if (window.gsap && window.ScrollTrigger && !REDUCED && !AUTOMATED) {
    try {
      window.gsap.registerPlugin(window.ScrollTrigger, window.CustomEase);

      /* wire-speed bar fills as it scrolls into view */
      const wireFill = document.getElementById("wire-fill");
      const wireMbps = document.getElementById("wire-mbps");
      if (wireFill) {
        const readout = { v: 0 };
        window.gsap.to(readout, {
          v: 108, ease: "none",
          onUpdate: () => { if (wireMbps) wireMbps.textContent = String(Math.round(readout.v)); },
          scrollTrigger: { trigger: ".wire", start: "top 85%", end: "top 35%", scrub: 0.5 },
        });
        window.gsap.fromTo(wireFill, { width: "0%" }, {
          width: "96.4%", ease: "none",
          scrollTrigger: { trigger: ".wire", start: "top 85%", end: "top 35%", scrub: 0.5 },
        });
      }

      /* footer rings drift as the page settles into them */
      const rings = document.getElementById("foot-rings");
      if (rings) {
        window.gsap.to(rings, {
          rotate: 34, ease: "none", transformOrigin: "100% 100%",
          scrollTrigger: { trigger: ".site-foot", start: "top bottom", end: "bottom bottom", scrub: 0.8 },
        });
      }
    } catch (e) {
      console.warn("scrolltrigger scrubs unavailable", e);
    }
  } else {
    // no scrub engine: settle the wire readout so the section isn't blank
    const wireMbps = document.getElementById("wire-mbps");
    const wireFill = document.getElementById("wire-fill");
    if (wireMbps) wireMbps.textContent = "108";
    if (wireFill) wireFill.style.width = "96.4%";
  }

  /* hero: lazy three.js — never under reduced motion or automation */
  if (!REDUCED && !AUTOMATED) {
    import("./hero3d.js").catch(() => { /* CSS fallback remains */ });
  }
}
initMotion();

/* ------------------------------------------------------- smooth anchors */
document.querySelectorAll('a[href^="#"]').forEach((a) => {
  a.addEventListener("click", (e) => {
    const id = a.getAttribute("href").slice(1);
    if (!id) return;
    const target = document.getElementById(id);
    if (!target) return;
    e.preventDefault();
    const lenis = window.__todokeLenis;
    if (lenis) lenis.scrollTo(target, { offset: -76 });
    else target.scrollIntoView({ behavior: REDUCED ? "auto" : "smooth", block: "start" });
    history.pushState(null, "", "#" + id);
  });
});

/* ------------------------------------------------------- scrollspy nav */
const spySections = ["features", "how", "measured", "questions"]
  .map((id) => ({ id, el: document.getElementById(id) }))
  .filter((s) => s.el);
const spyLinks = new Map(
  spySections.map((s) => {
    const link = document.querySelector(`.site-nav__link[href="#${s.id}"]`);
    return link ? [s.id, link] : [s.id, null];
  })
);
if ("IntersectionObserver" in window && spySections.length) {
  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        const link = spyLinks.get(e.target.id);
        if (link) link.classList.toggle("is-active", e.isIntersecting);
      }
    },
    { rootMargin: "-45% 0px -45% 0px" }
  );
  spySections.forEach((s) => io.observe(s.el));
}

/* ---------------------------------------------- FAQ: one open at a time */
const faqItems = [...document.querySelectorAll(".faq__item")];
faqItems.forEach((item) => {
  item.addEventListener("toggle", () => {
    if (item.open) faqItems.forEach((other) => { if (other !== item) other.open = false; });
  });
});

/* ----------------------------------------------------- magnetic buttons */
if (FINE && !REDUCED && !AUTOMATED) {
  document.querySelectorAll("[data-magnet]").forEach((btn) => {
    btn.addEventListener("pointermove", (e) => {
      const r = btn.getBoundingClientRect();
      const dx = (e.clientX - r.left - r.width / 2) / (r.width / 2);
      const dy = (e.clientY - r.top - r.height / 2) / (r.height / 2);
      btn.style.transform = `translate(${(dx * 6).toFixed(1)}px, ${(dy * 5).toFixed(1)}px)`;
    });
    btn.addEventListener("pointerleave", () => { btn.style.transform = ""; });
  });
}
