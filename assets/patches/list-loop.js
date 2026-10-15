// list-loop: scrolling past the end of a list wraps to the other end.
// Wraps the four stock commander handlers of List2Ctrl instead of replacing the
// 475 KB List2Ctrl.js. Verify on hardware. Behaviour studied from MZD-AIO-TI (GPL-3.0).
(function () {
  var proto = List2Ctrl.prototype;
  function edgeIndex(items, fromEnd) {
    var i;
    if (fromEnd) {
      for (i = items.length - 1; i >= 0; i--) { if (!items[i].disabled) { return i; } }
    } else {
      for (i = 0; i < items.length; i++) { if (!items[i].disabled) { return i; } }
    }
    return -1;
  }
  function wrap(name, toEnd) {
    var stock = proto[name];
    if (typeof stock !== "function" || stock._mzdTweaks) { return; }
    var patched = function () {
      var eligible = !this._inLetterIndexMulticontroller && !this._inListReorder;
      var before = eligible ? this._getFocussedIndex() : null;
      var result = stock.apply(this, arguments);
      if (eligible && this._getFocussedIndex() === before && this.dataList && this.dataList.items) {
        var target = edgeIndex(this.dataList.items, toEnd);
        if (target >= 0 && target !== before) {
          this._manageFocus(target);
          return "consumed";
        }
      }
      return result;
    };
    patched._mzdTweaks = true;
    proto[name] = patched;
  }
  wrap("_handleMCCW", false);
  wrap("_handleMCDown", false);
  wrap("_handleMCCCW", true);
  wrap("_handleMCUp", true);
})();
