// The page unchanged.
uniform sampler2D u_page;
I vec2 v_tc;

void main() {
    f_color = texture2D(u_page, v_tc);
}
