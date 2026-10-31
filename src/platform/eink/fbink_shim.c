// A fixed C ABI over FBInk, so Rust never mirrors FBInkConfig's layout.
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#include "fbink.h"

static int fbfd = -1;
static FBInkConfig cfg;

static FBInkState state;

int retsurf_eink_open(uint32_t* width, uint32_t* height, uint32_t* dpi) {
	fbfd = fbink_open();
	if (fbfd < 0) {
		return -1;
	}
	memset(&cfg, 0, sizeof(cfg));
	cfg.is_quiet     = true;
	cfg.ignore_alpha = true;
	if (fbink_init(fbfd, &cfg) < 0) {
		fbink_close(fbfd);
		fbfd = -1;
		return -1;
	}
	fbink_get_state(&cfg, &state);
	*width  = state.view_width;
	*height = state.view_height;
	*dpi    = state.screen_dpi;
	return 0;
}

// How the touch panel's axes map onto the view; valid once open succeeded.
void retsurf_eink_touch_quirks(bool* swap_axes, bool* mirror_x, bool* mirror_y) {
	*swap_axes = state.touch_swap_axes;
	*mirror_x  = state.touch_mirror_x;
	*mirror_y  = state.touch_mirror_y;
}

// Draws `gray` (one byte per pixel, `w` bytes a row) at (x, y) in view pixels as
// a partial refresh; returns FBInk's status.
int retsurf_eink_draw(const uint8_t* gray, int w, int h, int x, int y) {
	if (fbfd < 0) {
		return -1;
	}
	cfg.is_flashing = false;
	return fbink_print_raw_data(fbfd, gray, w, h, (size_t) w * (size_t) h, (short int) x, (short int) y, &cfg);
}

// A flashing full-panel refresh of what the framebuffer already holds.
int retsurf_eink_flash(void) {
	if (fbfd < 0) {
		return -1;
	}
	cfg.is_flashing = true;
	return fbink_refresh(fbfd, 0, 0, 0, 0, &cfg);
}

void retsurf_eink_close(void) {
	if (fbfd >= 0) {
		fbink_close(fbfd);
		fbfd = -1;
	}
}
