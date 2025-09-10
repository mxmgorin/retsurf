use crate::config::InterfaceConfig;
use webrender_api::{
    units::{DeviceIntSize, LayoutPoint, LayoutRect, LayoutSideOffsets, LayoutSize},
    BorderDetails, BorderRadius, BorderSide, BorderStyle, ClipMode, ColorF, CommonItemProperties,
    DisplayListBuilder, NormalBorder, PipelineId, PrimitiveFlags, SpaceAndClipInfo,
};

pub struct AppRender {
    _video_subsystem: sdl2::VideoSubsystem,
    _gl_ctx: sdl2::video::GLContext,
    window: sdl2::video::Window,
    renderer: webrender::Renderer,
    device_size: DeviceIntSize,
    builder: DisplayListBuilder,
    pipeline_id: PipelineId,
}

impl AppRender {
    pub fn new(sdl: &sdl2::Sdl, config: &InterfaceConfig) -> Self {
        let video_subsystem = sdl.video().unwrap();
        let gl_attr = video_subsystem.gl_attr();
        gl_attr.set_context_profile(sdl2::video::GLProfile::GLES);
        gl_attr.set_context_version(3, 0);

        let window = video_subsystem
            .window("Refsurf", config.width, config.height)
            .opengl()
            .resizable()
            .build()
            .unwrap();

        let gl_ctx = window.gl_create_context().unwrap();

        let debug_flags = webrender::DebugFlags::ECHO_DRIVER_MESSAGES;
        let opts = webrender::WebRenderOptions {
            resource_override_path: None,
            precache_flags: webrender::ShaderPrecacheFlags::empty(),
            clear_color: webrender_api::ColorF::new(0.3, 0.0, 0.0, 1.0),
            debug_flags,
            //allow_texture_swizzling: false,
            ..webrender::WebRenderOptions::default()
        };
        let gl = unsafe {
            gleam::gl::GlesFns::load_with(|symbol| {
                video_subsystem.gl_get_proc_address(symbol) as *const _
            })
        };

        let notifier = Box::new(AppRenderNotifier::new());
        let (renderer, sender) =
            webrender::create_webrender_instance(gl.clone(), notifier, opts, None).unwrap();
        let mut api = sender.create_api();
        let device_size =
            webrender_api::units::DeviceIntSize::new(config.width as i32, config.height as i32);
        let document_id = api.add_document(device_size);

        let epoch = webrender_api::Epoch(0);
        let pipeline_id = webrender_api::PipelineId(0, 0);
        let mut builder = webrender_api::DisplayListBuilder::new(pipeline_id);
        builder.begin();

        let mut obj = Self {
            _video_subsystem: video_subsystem,
            _gl_ctx: gl_ctx,
            window,
            renderer,
            device_size,
            builder,
            pipeline_id,
        };

        obj.render();

        let mut txn = webrender::Transaction::new();
        txn.set_display_list(epoch, obj.builder.end());
        txn.set_root_pipeline(pipeline_id);
        txn.generate_frame(0, true, webrender_api::RenderReasons::empty());
        api.send_transaction(document_id, txn);

        obj
    }

    fn render(&mut self) {
        let content_bounds = LayoutRect::from_size(LayoutSize::new(800.0, 600.0));
        let root_space_and_clip = SpaceAndClipInfo::root_scroll(self.pipeline_id);
        let spatial_id = root_space_and_clip.spatial_id;

        self.builder.push_simple_stacking_context(
            content_bounds.min,
            spatial_id,
            PrimitiveFlags::IS_BACKFACE_VISIBLE,
        );

        let complex = webrender_api::ComplexClipRegion::new(
            (50, 50).to(150, 150),
            BorderRadius::uniform(20.0),
            ClipMode::Clip,
        );
        let clip_id = self
            .builder
            .define_clip_rounded_rect(root_space_and_clip.spatial_id, complex);
        let clip_chain_id = self.builder.define_clip_chain(None, [clip_id]);

        self.builder.push_rect(
            &CommonItemProperties::new(
                (100, 100).to(200, 200),
                SpaceAndClipInfo {
                    spatial_id,
                    clip_chain_id,
                },
            ),
            (100, 100).to(200, 200),
            ColorF::new(0.0, 1.0, 0.0, 1.0),
        );

        self.builder.push_rect(
            &CommonItemProperties::new(
                (250, 100).to(350, 200),
                SpaceAndClipInfo {
                    spatial_id,
                    clip_chain_id,
                },
            ),
            (250, 100).to(350, 200),
            ColorF::new(0.0, 1.0, 0.0, 1.0),
        );
        let border_side = BorderSide {
            color: ColorF::new(0.0, 0.0, 1.0, 1.0),
            style: BorderStyle::Groove,
        };
        let border_widths = LayoutSideOffsets::new_all_same(10.0);
        let border_details = BorderDetails::Normal(NormalBorder {
            top: border_side,
            right: border_side,
            bottom: border_side,
            left: border_side,
            radius: BorderRadius::uniform(20.0),
            do_aa: true,
        });

        let bounds = (100, 100).to(200, 200);
        self.builder.push_border(
            &CommonItemProperties::new(
                bounds,
                webrender_api::SpaceAndClipInfo {
                    spatial_id,
                    clip_chain_id,
                },
            ),
            bounds,
            border_widths,
            border_details,
        );

        self.builder.pop_stacking_context();
    }

    pub fn show(&mut self) {
        self.renderer.update();
        self.renderer.render(self.device_size, 0).unwrap();
        self.window.gl_swap_window();
    }
}

struct AppRenderNotifier;
impl AppRenderNotifier {
    pub fn new() -> Self {
        Self {}
    }
}

impl webrender_api::RenderNotifier for AppRenderNotifier {
    fn clone(&self) -> Box<dyn webrender_api::RenderNotifier> {
        Box::new(AppRenderNotifier)
    }

    fn wake_up(&self, _composite_needed: bool) {}

    fn new_frame_ready(
        &self,
        _: webrender_api::DocumentId,
        _publish_id: webrender_api::FramePublishId,
        _params: &webrender_api::FrameReadyParams,
    ) {
    }
}
#[allow(dead_code)]
pub trait HandyDandyRectBuilder {
    fn to(&self, x2: i32, y2: i32) -> LayoutRect;
    fn by(&self, w: i32, h: i32) -> LayoutRect;
}
// Allows doing `(x, y).to(x2, y2)` or `(x, y).by(width, height)` with i32
// values to build a f32 LayoutRect
impl HandyDandyRectBuilder for (i32, i32) {
    fn to(&self, x2: i32, y2: i32) -> LayoutRect {
        LayoutRect::from_origin_and_size(
            LayoutPoint::new(self.0 as f32, self.1 as f32),
            LayoutSize::new((x2 - self.0) as f32, (y2 - self.1) as f32),
        )
    }

    fn by(&self, w: i32, h: i32) -> LayoutRect {
        LayoutRect::from_origin_and_size(
            LayoutPoint::new(self.0 as f32, self.1 as f32),
            LayoutSize::new(w as f32, h as f32),
        )
    }
}
