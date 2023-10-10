use std::sync::Arc;
use vulkano::buffer::{BufferUsage, CpuAccessibleBuffer};
use vulkano::command_buffer::{AutoCommandBufferBuilder, DynamicState};
use vulkano::device::{Device, DeviceExtensions, Features};
use vulkano::instance::{Instance, InstanceExtensions, PhysicalDevice};
use vulkano::pipeline::GraphicsPipeline;
use vulkano::render_pass::{Framebuffer, FramebufferAbstract, Subpass};
use vulkano::single_pass_renderpass;
use vulkano::swapchain::{AcquireError, PresentMode, SurfaceTransform, Swapchain};
use vulkano::sync::{FlushError, GpuFuture, Semaphore};
use vulkano_win::VkSurfaceBuild;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() {
    // 创建一个事件循环
    let event_loop = EventLoop::new();
    // 创建一个窗口
    let window = WindowBuilder::new()
        .with_title("Vulkano Triangle Example")
        .build_vk_surface(&event_loop, Instance::new(None, &Features::none(), &InstanceExtensions::none(), ()).unwrap())
        .unwrap();

    // 选择物理设备
    let physical = PhysicalDevice::enumerate(&instance).next().unwrap();
    // 选择支持的队列族
    let queue_family = physical.queue_families().find(|&q| q.supports_graphics()).unwrap();
    // 创建设备和队列
    let (device, mut queues) = {
        Device::new(
            physical,
            &Features::none(),
            &DeviceExtensions::none(),
            [(queue_family, 0.5)].iter().cloned(),
        )
            .unwrap()
    };
    let queue = queues.next().unwrap();

    // 创建交换链
    let (mut swapchain, images) = {
        let caps = surface.capabilities(physical).unwrap();
        let dimensions = caps.current_extent.unwrap_or([1024, 768]);
        Swapchain::new(
            device.clone(),
            surface.clone(),
            caps.min_image_count,
            vulkano::format::Format::B8G8R8A8Srgb,
            dimensions,
            1,
            caps.supported_usage_flags,
            &queue,
            SurfaceTransform::Identity,
            vulkano::swapchain::CompositeAlpha::Opaque,
            PresentMode::Fifo,
            vulkano::swapchain::FullscreenExclusive::Default,
            true,
            vulkano::swapchain::ColorSpace::SrgbNonLinear,
        )
            .unwrap()
    };

    // 创建渲染通道
    let render_pass = Arc::new(
        single_pass_renderpass!(device.clone(),
            attachments: {
                color: {
                    load: Clear,
                    store: Store,
                    format: swapchain.format(),
                    samples: 1,
                }
            },
            pass: {
                color: [color],
                depth_stencil: {}
            }
        )
            .unwrap(),
    );

    // 创建帧缓冲
    let framebuffers = images
        .iter()
        .map(|image| {
            Arc::new(
                Framebuffer::start(render_pass.clone())
                    .add(image.clone())
                    .unwrap()
                    .build()
                    .unwrap(),
            ) as Arc<dyn FramebufferAbstract + Send + Sync>
        })
        .collect::<Vec<_>>();

    // 创建顶点缓冲
    #[derive(Default, Debug, Clone)]
    struct Vertex {
        position: [f32; 2],
    }
    vulkano::impl_vertex!(Vertex, position);
    let vertex_buffer = CpuAccessibleBuffer::from_iter(
        device.clone(),
        BufferUsage::all(),
        false,
        [
            Vertex {
                position: [-0.5, -0.25],
            },
            Vertex {
                position: [0.0, 0.5],
            },
            Vertex {
                position: [0.25, -0.1],
            },
        ]
            .iter()
            .cloned(),
    )
        .unwrap();

    // 创建顶点着色器
    mod vs {
        vulkano_shaders::shader! {
            ty: "vertex",
            src: "
                #version 450

                layout(location = 0) in vec2 position;

                void main() {
                    gl_Position = vec4(position, 0.0, 1.0);
                }
            "
        }
    }
    let vs = vs::Shader::load(device.clone()).unwrap();

    // 创建片段着色器
    mod fs {
        vulkano_shaders::shader! {
            ty: "fragment",
            src: "
                #version 450

                layout(location = 0) out vec4 f_color;

                void main() {
                    f_color = vec4(1.0, 0.0, 0.0, 1.0);
                }
            "
        }
    }
    let fs = fs::Shader::load(device.clone()).unwrap();

    // 创建管线
    let pipeline = Arc::new(
        GraphicsPipeline::start()
            .vertex_input_single_buffer::<Vertex>()
            .vertex_shader(vs.main_entry_point(), ())
            .triangle_list()
            .viewports_dynamic_scissors_irrelevant(1)
            .fragment_shader(fs.main_entry_point(), ())
            .render_pass(Subpass::from(render_pass.clone(), 0).unwrap())
            .build(device.clone())
            .unwrap(),
    );

    // 创建命令缓冲
    let mut builder = AutoCommandBufferBuilder::primary_one_time_submit(device.clone(), queue.family()).unwrap();
    builder
        .begin_render_pass(framebuffers[0].clone(), false, vec![[0.0, 0.0, 1.0, 1.0].into()])
        .unwrap()
        .draw(
            pipeline.clone(),
            &DynamicState::none(),
            vec![vertex_buffer.clone()],
            (),
            (),
        )
        .unwrap()
        .end_render_pass()
        .unwrap();
    let command_buffer = builder.build().unwrap();

    // 创建信号量
    let (image_available, finished) = {
        let semaphore = Semaphore::new(device.clone()).unwrap();
        (semaphore.clone(), semaphore)
    };

    // 主循环
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
                return;
            }
            Event::RedrawRequested(_) => {
                // 获取下一个图像
                let (image_index, acquire_future) =
                    match vulkano::swapchain::acquire_next_image(swapchain.clone(), None) {
                        Ok(r) => r,
                        Err(AcquireError::OutOfDate) => {
                            recreate_swapchain = true;
                            return;
                        }
                        Err(e) => panic!("Failed to acquire next image: {:?}", e),
                    };

                // 提交命令缓冲
                let command_buffer = AutoCommandBufferBuilder::primary_one_time_submit(device.clone(), queue.family())
                    .unwrap()
                    .execute_commands(command_buffer.clone())
                    .unwrap()
                    .build()
                    .unwrap();
                let future = previous_frame_end
                    .join(acquire_future)
                    .then_execute(queue.clone(), command_buffer)
                    .unwrap()
                    .then_swapchain_present(queue.clone(), swapchain.clone(), image_index)
                    .then_signal_fence_and_flush();
                match future {
                    Ok(future) => {
                        previous_frame_end = Box::new(future) as Box<dyn GpuFuture>;
                    }
                    Err(FlushError::OutOfDate) => {
                        recreate_swapchain = true;
                        previous_frame_end = Box::new(vulkano::sync::now(device.clone())) as Box<dyn GpuFuture>;
                    }
                    Err(e) => {
                        println!("Failed to flush future: {:?}", e);
                        previous_frame_end = Box::new(vulkano::sync::now(device.clone())) as Box<dyn GpuFuture>;
                    }
                }
            }
            _ => (),
        }
    });
}
