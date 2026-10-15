// no-disclaimer: accept the boot disclaimer automatically.
// Wraps the stock systemApp.prototype._displayedDisclaimer instead of replacing
// systemApp.js, so the stock file stays intact on every firmware build.
// Verify on hardware. Behaviour studied from MZD-AIO-TI (GPL-3.0).
(function () {
  var proto = systemApp.prototype;
  var stock = proto._displayedDisclaimer;
  if (typeof stock !== "function" || stock._mzdTweaks) { return; }
  var patched = function () {
    stock.apply(this, arguments);
    framework.sendEventToMmui("common", "Global.Yes");
  };
  patched._mzdTweaks = true;
  proto._displayedDisclaimer = patched;
})();
