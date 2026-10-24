/* Dasho — dasho.tech
   Progressive enhancement only: the page is complete without it. */
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
})();
