// Caps the texture units WebGL reports at WEBGL_TEXTURE_UNITS. Batching engines
// build one shader sampling every unit, which a CPU rasterizer compiles slowly.
(function () {
  "use strict";
  var UNITS = WEBGL_TEXTURE_UNITS;
  [window.WebGLRenderingContext, window.WebGL2RenderingContext].forEach(function (C) {
    if (!C) { return; }
    var getParameter = C.prototype.getParameter;
    C.prototype.getParameter = function (pname) {
      var value = getParameter.call(this, pname);
      return pname === this.MAX_TEXTURE_IMAGE_UNITS ? Math.min(value, UNITS) : value;
    };
  });
})();
