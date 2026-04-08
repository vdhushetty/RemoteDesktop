use crate::frame::CapturedFrame;
use crate::{CaptureError, Capturer};

use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication,
    DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::Interface;

pub struct DxgiCapturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    width: u32,
    height: u32,
}

impl DxgiCapturer {
    pub fn new() -> Result<Self, CaptureError> {
        unsafe {
            // Create D3D11 device
            let mut device = None;
            let mut context = None;

            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| CaptureError::Init(format!("D3D11CreateDevice failed: {e}")))?;

            let device = device.ok_or_else(|| CaptureError::Init("No D3D11 device".into()))?;
            let context =
                context.ok_or_else(|| CaptureError::Init("No D3D11 context".into()))?;

            // Get DXGI output
            let dxgi_device: windows::Win32::Graphics::Dxgi::IDXGIDevice =
                device.cast().map_err(|e| CaptureError::Init(format!("Cast to IDXGIDevice: {e}")))?;

            let adapter = dxgi_device
                .GetAdapter()
                .map_err(|e| CaptureError::Init(format!("GetAdapter: {e}")))?;

            let output = adapter
                .EnumOutputs(0)
                .map_err(|e| CaptureError::Init(format!("EnumOutputs: {e}")))?;

            let output1: IDXGIOutput1 = output
                .cast()
                .map_err(|e| CaptureError::Init(format!("Cast to IDXGIOutput1: {e}")))?;

            let desc = output1
                .GetDesc()
                .map_err(|e| CaptureError::Init(format!("GetDesc: {e}")))?;

            let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
            let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;

            // Create output duplication
            let duplication = output1
                .DuplicateOutput(&device)
                .map_err(|e| CaptureError::Init(format!("DuplicateOutput: {e}")))?;

            tracing::info!(width, height, "initialized DXGI screen capture");

            Ok(Self {
                device,
                context,
                duplication,
                width,
                height,
            })
        }
    }
}

impl Capturer for DxgiCapturer {
    fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource = None;

            // Acquire next frame (100ms timeout)
            self.duplication
                .AcquireNextFrame(100, &mut frame_info, &mut resource)
                .map_err(|e| CaptureError::Capture(format!("AcquireNextFrame: {e}")))?;

            let resource =
                resource.ok_or_else(|| CaptureError::Capture("No frame resource".into()))?;

            let texture: ID3D11Texture2D = resource
                .cast()
                .map_err(|e| CaptureError::Capture(format!("Cast to texture: {e}")))?;

            // Create staging texture for CPU read
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = Default::default();
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0.into();
            desc.MiscFlags = Default::default();

            let staging = self
                .device
                .CreateTexture2D(&desc, None)
                .map_err(|e| CaptureError::Capture(format!("CreateTexture2D: {e}")))?;

            // Copy to staging
            self.context.CopyResource(&staging, &texture);

            // Map and read pixels
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| CaptureError::Capture(format!("Map: {e}")))?;

            let stride = mapped.RowPitch;
            let data_size = (stride * self.height) as usize;
            let data =
                std::slice::from_raw_parts(mapped.pData as *const u8, data_size).to_vec();

            self.context.Unmap(&staging, 0);

            // Release frame
            self.duplication
                .ReleaseFrame()
                .map_err(|e| CaptureError::Capture(format!("ReleaseFrame: {e}")))?;

            Ok(CapturedFrame::new(data, self.width, self.height, stride))
        }
    }

    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}
