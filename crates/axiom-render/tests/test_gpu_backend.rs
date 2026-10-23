use axiom_render::framebuffer::FrameBuffer;
use axiom_render::gpu_backend::{
    GpuBackendKind, GpuPresenter, GpuPresenterConfig, PresentStats, ScalingMode,
};

#[test]
fn test_gpu_presenter_64b_alignment() {
    assert_eq!(core::mem::size_of::<GpuPresenterConfig>(), 64);
    assert_eq!(core::mem::align_of::<GpuPresenterConfig>(), 64);

    assert_eq!(core::mem::size_of::<PresentStats>(), 64);
    assert_eq!(core::mem::align_of::<PresentStats>(), 64);
}

#[test]
fn test_viewport_scaling_and_letterbox() {
    let mut config = GpuPresenterConfig::default();
    config.viewport_w = 1280;
    config.viewport_h = 720; // 16:9 display
    config.scaling = ScalingMode::FitAspectLetterbox;

    let presenter = GpuPresenter::new(config);
    // Source framebuffer: 640x480 (4:3 aspect)
    let rect = presenter.compute_destination_viewport(640, 480);

    // Height fits to 720, width scales by 1.5x to 960 (4:3 ratio preserved)
    assert_eq!(rect.height, 720);
    assert_eq!(rect.width, 960);
    // Centered horizontally: (1280 - 960) / 2 = 160
    assert_eq!(rect.x, 160);
    assert_eq!(rect.y, 0);

    // Test Integer 2x:
    let mut config_2x = config;
    config_2x.scaling = ScalingMode::Integer2x;
    let presenter_2x = GpuPresenter::new(config_2x);
    let rect_2x = presenter_2x.compute_destination_viewport(320, 240);
    assert_eq!(rect_2x.width, 640);
    assert_eq!(rect_2x.height, 480);
    assert_eq!(rect_2x.x, (1280 - 640) / 2);
    assert_eq!(rect_2x.y, (720 - 480) / 2);
}

#[test]
fn test_present_all_backends() {
    let fb = FrameBuffer::new(320, 240);
    let backends = [
        GpuBackendKind::SoftwareCpu,
        GpuBackendKind::Vulkan,
        GpuBackendKind::DirectX12,
        GpuBackendKind::Metal,
        GpuBackendKind::WebGpu,
    ];

    for b in backends {
        let mut config = GpuPresenterConfig::default();
        config.backend = b;
        let mut presenter = GpuPresenter::new(config);

        assert!(presenter.present(&fb));
        assert_eq!(presenter.stats.total_presented_frames, 1);
        assert!(presenter.stats.present_time_us > 0);

        if b.is_hardware_accelerated() {
            assert!(presenter.stats.present_time_us < 100);
        } else {
            assert_eq!(presenter.stats.present_time_us, 150);
        }
    }
}

#[test]
fn test_voxel_compute_raymarch_dispatch() {
    use axiom_render::gpu_backend::VoxelComputeDispatch64;

    assert_eq!(core::mem::size_of::<VoxelComputeDispatch64>(), 64);
    assert_eq!(core::mem::align_of::<VoxelComputeDispatch64>(), 64);

    let mut dispatch = VoxelComputeDispatch64::default();
    let compute_time = dispatch.dispatch(704, 544);
    assert!(compute_time > 0);
    assert_eq!(dispatch.total_dispatches, 1);
    assert_eq!(dispatch.dispatch_x, 88);
    assert_eq!(dispatch.dispatch_y, 68);
}
