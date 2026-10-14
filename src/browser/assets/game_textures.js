// Shrink the textures a WebGL game uploads, so its art fits a handheld's GPU
// memory. A texture whose larger side exceeds LIMIT is halved until it fits;
// texture coordinates are normalized, so the smaller one samples the same way.
// A sub-upload into a shrunk texture is scaled the same way, so an atlas stays
// consistent. Left alone: compressed textures, texStorage2D, and data in a
// buffer object. Runs as a user script, before any page script makes a context.
// LIMIT is the largest side kept, in px; FILTER is "smooth" (2x2 average,
// weighted by alpha) or "sharp" (every other pixel, for pixel art). Under
// "smooth" a shrunk texture also samples linearly whatever the game asks: drawn
// back up at nearest, each of its pixels is a 2x2 block and text breaks apart.
(function () {
    const LIMIT = GAME_TEXTURE_LIMIT;
    const FILTER = "GAME_TEXTURE_FILTER";
    const TEXTURE_2D = 0x0de1;
    const TEXTURE0 = 0x84c0;
    const RGBA = 0x1908;
    const RGBA8 = 0x8058;
    const UNSIGNED_BYTE = 0x1401;
    const MAG_FILTER = 0x2800;
    const MIN_FILTER = 0x2801;
    // Each nearest filter and its linear counterpart.
    const LINEAR_OF = new Map([
        [0x2600, 0x2601], // NEAREST -> LINEAR
        [0x2700, 0x2701], // NEAREST_MIPMAP_NEAREST -> LINEAR_MIPMAP_NEAREST
        [0x2702, 0x2703], // NEAREST_MIPMAP_LINEAR -> LINEAR_MIPMAP_LINEAR
    ]);
    const stats = { shrunk: 0, kept: 0, skipped: 0, scaledSubs: 0, bytesIn: 0, bytesOut: 0 };
    window.__retsurfTextures = stats;

    // How many halvings each shrunk texture took, so its other uploads match.
    const shifts = new WeakMap();
    // The filters each texture asked for, to upgrade once it is shrunk.
    const filters = new WeakMap();
    // Each context's active unit and its TEXTURE_2D binding per unit, tracked
    // here: asking the context would be a synchronous round trip per call.
    const contexts = new WeakMap();

    function stateOf(gl) {
        let state = contexts.get(gl);
        if (!state) {
            state = { unit: 0, units: new Map() };
            contexts.set(gl, state);
        }
        return state;
    }

    function boundTexture(gl) {
        const state = stateOf(gl);
        return state.units.get(state.unit) || null;
    }

    // Halvings for a w x h level 0 to fit LIMIT; 0 when it already does.
    function shiftFor(w, h) {
        let shift = 0;
        while (Math.max(w, h) >> shift > LIMIT) {
            shift++;
        }
        return shift;
    }

    function scaled(size, shift) {
        return Math.max(1, Math.ceil(size / (1 << shift)));
    }

    // One halving of RGBA pixels; odd sizes reuse their last row and column.
    function halve(src, w, h) {
        const nw = Math.max(1, (w + 1) >> 1);
        const nh = Math.max(1, (h + 1) >> 1);
        const out = new Uint8Array(nw * nh * 4);
        for (let y = 0; y < nh; y++) {
            const y0 = y * 2;
            const y1 = Math.min(y0 + 1, h - 1);
            for (let x = 0; x < nw; x++) {
                const x0 = x * 2;
                const o = (y * nw + x) * 4;
                if (FILTER === "sharp") {
                    const i = (y0 * w + x0) * 4;
                    out[o] = src[i];
                    out[o + 1] = src[i + 1];
                    out[o + 2] = src[i + 2];
                    out[o + 3] = src[i + 3];
                    continue;
                }
                const x1 = Math.min(x0 + 1, w - 1);
                let r = 0, g = 0, b = 0, a = 0;
                for (const i of [(y0 * w + x0) * 4, (y0 * w + x1) * 4, (y1 * w + x0) * 4, (y1 * w + x1) * 4]) {
                    const alpha = src[i + 3];
                    r += src[i] * alpha;
                    g += src[i + 1] * alpha;
                    b += src[i + 2] * alpha;
                    a += alpha;
                }
                // A fully transparent block keeps black, which nothing shows.
                if (a > 0) {
                    out[o] = r / a;
                    out[o + 1] = g / a;
                    out[o + 2] = b / a;
                }
                out[o + 3] = a >> 2;
            }
        }
        return { data: out, w: nw, h: nh };
    }

    function shrinkPixels(pixels, w, h, shift) {
        let image = { data: new Uint8Array(pixels.buffer, pixels.byteOffset, w * h * 4), w, h };
        for (let i = 0; i < shift; i++) {
            image = halve(image.data, image.w, image.h);
        }
        return image;
    }

    // An image, canvas or bitmap drawn into a canvas of the shrunk size.
    function shrinkSource(source, shift) {
        const w = source.naturalWidth || source.videoWidth || source.width;
        const h = source.naturalHeight || source.videoHeight || source.height;
        const canvas = document.createElement("canvas");
        canvas.width = scaled(w, shift);
        canvas.height = scaled(h, shift);
        const ctx = canvas.getContext("2d");
        ctx.imageSmoothingEnabled = FILTER !== "sharp";
        ctx.drawImage(source, 0, 0, canvas.width, canvas.height);
        return canvas;
    }

    function linear(param) {
        return FILTER === "sharp" ? param : LINEAR_OF.get(param) || param;
    }

    // Level 0 decides a texture's scale; its other levels follow it. A shrunk
    // one gets the linear form of the filters it asked for.
    function markLevel0(gl, texParameteri, texture, level, shift) {
        if (level !== 0) {
            return;
        }
        shifts.set(texture, shift);
        if (shift && FILTER !== "sharp") {
            const asked = filters.get(texture) || {};
            texParameteri.call(gl, TEXTURE_2D, MAG_FILTER, linear(asked[MAG_FILTER] || 0x2601));
            if (asked[MIN_FILTER]) {
                texParameteri.call(gl, TEXTURE_2D, MIN_FILTER, linear(asked[MIN_FILTER]));
            }
        }
    }

    function isPixels(value) {
        return ArrayBuffer.isView(value) && value.BYTES_PER_ELEMENT === 1;
    }

    function plainRgba(internal, format, type) {
        return (internal === RGBA || internal === RGBA8) && format === RGBA && type === UNSIGNED_BYTE;
    }

    function wrap(proto) {
        const texParameteri = proto.texParameteri;
        proto.texParameteri = function (target, pname, param) {
            const texture = target === TEXTURE_2D ? boundTexture(this) : null;
            if (!texture || (pname !== MAG_FILTER && pname !== MIN_FILTER)) {
                return texParameteri.apply(this, arguments);
            }
            const asked = filters.get(texture) || {};
            asked[pname] = param;
            filters.set(texture, asked);
            const shrunk = (shifts.get(texture) || 0) > 0;
            return texParameteri.call(this, target, pname, shrunk ? linear(param) : param);
        };
        const texParameterf = proto.texParameterf;
        proto.texParameterf = function (target, pname, param) {
            if (pname === MAG_FILTER || pname === MIN_FILTER) {
                return proto.texParameteri.call(this, target, pname, Math.round(param));
            }
            return texParameterf.apply(this, arguments);
        };

        const activeTexture = proto.activeTexture;
        proto.activeTexture = function (unit) {
            stateOf(this).unit = unit - TEXTURE0;
            return activeTexture.apply(this, arguments);
        };

        const bindTexture = proto.bindTexture;
        proto.bindTexture = function (target, texture) {
            if (target === TEXTURE_2D) {
                const state = stateOf(this);
                state.units.set(state.unit, texture);
            }
            return bindTexture.apply(this, arguments);
        };

        const texImage2D = proto.texImage2D;
        proto.texImage2D = function (target, level, internal) {
            const texture = target === TEXTURE_2D ? boundTexture(this) : null;
            if (!texture) {
                return texImage2D.apply(this, arguments);
            }
            // (target, level, internal, format, type, source)
            if (arguments.length === 6 && typeof arguments[5] === "object" && !isPixels(arguments[5])) {
                const [, , , format, type, source] = arguments;
                const w = source.naturalWidth || source.videoWidth || source.width;
                const h = source.naturalHeight || source.videoHeight || source.height;
                const shift = level === 0 ? shiftFor(w, h) : shifts.get(texture) || 0;
                if (!shift || source instanceof HTMLVideoElement) {
                    markLevel0(this, texParameteri, texture, level, 0);
                    stats.kept++;
                    return texImage2D.apply(this, arguments);
                }
                markLevel0(this, texParameteri, texture, level, shift);
                stats.shrunk++;
                return texImage2D.call(this, target, level, internal, format, type, shrinkSource(source, shift));
            }
            // (target, level, internal, w, h, border, format, type, pixels[, offset])
            const [, , , w, h, border, format, type, pixels, offset] = arguments;
            const shift = level === 0 ? shiftFor(w, h) : shifts.get(texture) || 0;
            // No data is a texture filled later, piece by piece: kept whole.
            if (!shift || !isPixels(pixels)) {
                markLevel0(this, texParameteri, texture, level, 0);
                stats.kept++;
                return texImage2D.apply(this, arguments);
            }
            if (!plainRgba(internal, format, type)) {
                markLevel0(this, texParameteri, texture, level, 0);
                stats.skipped++;
                return texImage2D.apply(this, arguments);
            }
            markLevel0(this, texParameteri, texture, level, shift);
            const view = offset ? pixels.subarray(offset) : pixels;
            const image = shrinkPixels(view, w, h, shift);
            stats.shrunk++;
            stats.bytesIn += w * h * 4;
            stats.bytesOut += image.data.length;
            return texImage2D.call(this, target, level, internal, image.w, image.h, border, format, type, image.data);
        };

        const texSubImage2D = proto.texSubImage2D;
        proto.texSubImage2D = function (target, level, x, y) {
            const texture = target === TEXTURE_2D ? boundTexture(this) : null;
            const shift = texture ? shifts.get(texture) || 0 : 0;
            if (!shift) {
                return texSubImage2D.apply(this, arguments);
            }
            stats.scaledSubs++;
            const sx = x >> shift;
            const sy = y >> shift;
            // (target, level, x, y, format, type, source)
            if (arguments.length === 7 && typeof arguments[6] === "object" && !isPixels(arguments[6])) {
                const [, , , , format, type, source] = arguments;
                return texSubImage2D.call(this, target, level, sx, sy, format, type, shrinkSource(source, shift));
            }
            // (target, level, x, y, w, h, format, type, pixels[, offset])
            const [, , , , w, h, format, type, pixels, offset] = arguments;
            if (!isPixels(pixels) || format !== RGBA || type !== UNSIGNED_BYTE) {
                return texSubImage2D.apply(this, arguments);
            }
            const view = offset ? pixels.subarray(offset) : pixels;
            const image = shrinkPixels(view, w, h, shift);
            return texSubImage2D.call(this, target, level, sx, sy, image.w, image.h, format, type, image.data);
        };
    }

    for (const context of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
        if (context) {
            wrap(context.prototype);
        }
    }
})();
