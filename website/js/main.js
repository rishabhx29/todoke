/* Dasho — dasho.tech
   Progressive enhancement only: the page is complete without it. */
(function () {
  "use strict";

  document.documentElement.classList.add("js");

  var yearEl = document.getElementById("year");
  if (yearEl) yearEl.textContent = String(new Date().getFullYear());
})();
