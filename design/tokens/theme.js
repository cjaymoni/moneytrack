// Design-reference only: lets a screen be opened as ?theme=dark or ?theme=light.
// Not part of the shipped app — Flutter reads the platform brightness.
(function () {
  var t = new URLSearchParams(location.search).get('theme');
  if (t === 'dark' || t === 'light') document.documentElement.dataset.theme = t;
})();
