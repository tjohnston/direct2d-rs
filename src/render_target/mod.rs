use brush::Brush;
use directwrite::{TextFormat, TextLayout};
use enums::{AntialiasMode, BitmapInterpolationMode, DrawTextOptions, UncheckedEnum};
use error::Error;
use factory::Factory;
use geometry::Geometry;
use image::Bitmap;
use math::*;
use stroke_style::StrokeStyle;

use std::{mem, ptr};

use winapi::shared::winerror::SUCCEEDED;
use winapi::um::d2d1::{D2D1_TAG, ID2D1Factory, ID2D1RenderTarget};
use winapi::um::d2d1_1::ID2D1Factory1;
use winapi::um::dcommon::DWRITE_MEASURING_MODE_NATURAL;
use wio::com::ComPtr;
use wio::wide::ToWide;

#[doc(inline)]
pub use self::dxgi::DxgiSurfaceRenderTarget;
#[doc(inline)]
pub use self::generic::GenericRenderTarget;
#[doc(inline)]
pub use self::hwnd::HwndRenderTarget;

pub mod dxgi;
pub mod generic;
pub mod hwnd;

#[derive(Copy, Clone, Debug)]
pub struct RenderTag {
    pub loc: &'static str,
}

#[repr(C)]
struct RenderTagRaw(usize, usize);

#[macro_export]
#[doc(hidden)]
macro_rules! make_render_tag {
    () => {
        $crate::render_target::RenderTag {
            loc: concat!(file!(), ':', line!()),
        }
    };
}

#[macro_export]
/// Use this to set a checkpoint that will be returned if flush() or end_draw() returns an
/// error to help debug which part of the drawing code is causing the error.
///
/// ```
/// # #[macro_use] extern crate direct2d;
/// # extern crate direct3d11;
/// # extern crate dxgi;
/// # use direct2d::{DeviceContext, RenderTarget};
/// # use direct2d::brush::SolidColorBrush;
/// # use direct2d::image::Bitmap;
/// fn draw(context: &mut DeviceContext, target: &Bitmap) {
///     let brush = SolidColorBrush::create(&context)
///         .with_color(0xFF_7F_7F)
///         .build().unwrap();
///
///     context.begin_draw();
///     context.set_target(target);
///     context.clear(0xFF_FF_FF);
///
///     // Not sure which of these two lines could mess it up, so I set
///     // the render tag to be notified of the failure in the Err value.
///     set_render_tag!(context);
///     context.draw_line((10.0, 10.0), (20.0, 20.0), &brush, 2.0, None);
///
///     set_render_tag!(context);
///     context.draw_line((10.0, 20.0), (20.0, 10.0), &brush, 2.0, None);
///
///     match context.end_draw() {
///         Ok(_) => {/* cool */},
///         Err((err, Some(tag))) => {
///             panic!("Uh oh, rendering failed at {}: {}", tag.loc, err);
///         }
///         Err((err, None)) => {
///             panic!("Uh oh, rendering failed at an unknown location: {}", err);
///         }
///     }
/// }
/// # fn main() {
/// #     use direct2d::{Device, Factory};
/// #     use direct2d::enums::BitmapOptions;
/// #     use direct3d11::flags::{BindFlags, CreateDeviceFlags};
/// #     use dxgi::Format;
/// #     let (_, d3d, _) = direct3d11::device::Device::create()
/// #         .with_flags(CreateDeviceFlags::BGRA_SUPPORT)
/// #         .build()
/// #         .unwrap();
/// #     let tex = direct3d11::texture2d::Texture2D::create(&d3d)
/// #         .with_size(64, 64)
/// #         .with_format(Format::R8G8B8A8Unorm)
/// #         .with_bind_flags(BindFlags::RENDER_TARGET | BindFlags::SHADER_RESOURCE)
/// #         .build()
/// #         .unwrap();
/// #     let factory = Factory::new().unwrap();
/// #     let dev = Device::create(&factory, &d3d.as_dxgi()).unwrap();
/// #     let mut ctx = DeviceContext::create(&dev, false).unwrap();
/// #     let target = Bitmap::create(&ctx)
/// #         .with_dxgi_surface(&tex.as_dxgi())
/// #         .with_dpi(192.0, 192.0)
/// #         .with_options(BitmapOptions::TARGET)
/// #         .build()
/// #         .unwrap();
/// #     draw(&mut ctx, &target);
/// # }
/// ```
macro_rules! set_render_tag {
    ($rt:ident) => {
        $crate::render_target::RenderTarget::set_tag($rt, make_render_tag!());
    };
}

impl<'r, R> RenderTarget for &'r mut R
where
    R: RenderTarget + 'r,
{
    #[doc(hidden)]
    #[inline]
    unsafe fn rt<'a>(&self) -> &'a mut ID2D1RenderTarget {
        R::rt(*self)
    }
}

pub trait RenderTarget {
    #[doc(hidden)]
    #[inline]
    unsafe fn rt<'a>(&self) -> &'a mut ID2D1RenderTarget;

    #[doc(hidden)]
    #[inline]
    unsafe fn make_tag(tag1: D2D1_TAG, tag2: D2D1_TAG) -> Option<RenderTag> {
        if tag1 == 0 {
            None
        } else {
            let raw = RenderTagRaw(tag1 as usize, tag2 as usize);
            let tag = mem::transmute(raw);
            Some(tag)
        }
    }

    #[inline]
    fn as_generic(&self) -> GenericRenderTarget {
        unsafe {
            let ptr = self.rt();
            ptr.AddRef();
            GenericRenderTarget::from_raw(ptr)
        }
    }

    #[inline]
    fn get_factory(&mut self) -> Factory {
        unsafe {
            let mut ptr: *mut ID2D1Factory = ptr::null_mut();
            self.rt().GetFactory(&mut ptr);

            let ptr: ComPtr<ID2D1Factory1> = ComPtr::from_raw(ptr).cast().unwrap();
            Factory::from_raw(ptr.into_raw())
        }
    }

    #[inline]
    fn get_size(&self) -> SizeF {
        unsafe { SizeF(self.rt().GetSize()) }
    }

    #[inline]
    fn begin_draw(&mut self) {
        unsafe {
            self.rt().BeginDraw();
        }
    }

    #[inline]
    fn end_draw(&mut self) -> Result<(), (Error, Option<RenderTag>)> {
        let mut tag1 = 0;
        let mut tag2 = 0;
        unsafe {
            let result = self.rt().EndDraw(&mut tag1, &mut tag2);

            if SUCCEEDED(result) {
                Ok(())
            } else {
                let tag = Self::make_tag(tag1, tag2);
                Err((From::from(result), tag))
            }
        }
    }

    #[inline]
    fn set_tag(&mut self, tag: RenderTag) {
        unsafe {
            let RenderTagRaw(tag1, tag2) = mem::transmute(tag);
            self.rt().SetTags(tag1 as u64, tag2 as u64)
        };
    }

    #[inline]
    fn get_tag(&mut self) -> Option<RenderTag> {
        let mut tag1 = 0;
        let mut tag2 = 0;
        unsafe {
            self.rt().GetTags(&mut tag1, &mut tag2);
            Self::make_tag(tag1, tag2)
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), (Error, Option<RenderTag>)> {
        let mut tag1 = 0;
        let mut tag2 = 0;
        unsafe {
            let result = self.rt().Flush(&mut tag1, &mut tag2);

            if SUCCEEDED(result) {
                Ok(())
            } else {
                let tag = Self::make_tag(tag1, tag2);
                Err((From::from(result), tag))
            }
        }
    }

    #[inline]
    fn clear<C>(&mut self, color: C)
    where
        C: Into<ColorF>,
    {
        unsafe {
            self.rt().Clear(&color.into().0);
        }
    }

    #[inline]
    fn draw_line<P0, P1, B>(
        &mut self,
        p0: P0,
        p1: P1,
        brush: &B,
        stroke_width: f32,
        stroke_style: Option<&StrokeStyle>,
    ) where
        P0: Into<Point2F>,
        P1: Into<Point2F>,
        B: Brush,
    {
        unsafe {
            let stroke_style = match stroke_style {
                Some(s) => s.get_raw() as *mut _,
                None => ptr::null_mut(),
            };

            self.rt().DrawLine(
                p0.into().0,
                p1.into().0,
                brush.get_ptr(),
                stroke_width,
                stroke_style,
            )
        }
    }

    #[inline]
    fn draw_rectangle<R, B>(
        &mut self,
        rect: R,
        brush: &B,
        stroke_width: f32,
        stroke_style: Option<&StrokeStyle>,
    ) where
        R: Into<RectF>,
        B: Brush,
    {
        unsafe {
            let stroke_style = match stroke_style {
                Some(s) => s.get_raw() as *mut _,
                None => ptr::null_mut(),
            };

            self.rt()
                .DrawRectangle(&rect.into().0, brush.get_ptr(), stroke_width, stroke_style);
        }
    }

    #[inline]
    fn fill_rectangle<R, B>(&mut self, rect: R, brush: &B)
    where
        R: Into<RectF>,
        B: Brush,
    {
        unsafe {
            self.rt().FillRectangle(&rect.into().0, brush.get_ptr());
        }
    }

    #[inline]
    fn draw_rounded_rectangle<R, B>(
        &mut self,
        rect: R,
        brush: &B,
        stroke_width: f32,
        stroke_style: Option<&StrokeStyle>,
    ) where
        R: Into<RoundedRect>,
        B: Brush,
    {
        unsafe {
            let stroke_style = match stroke_style {
                Some(s) => s.get_raw() as *mut _,
                None => ptr::null_mut(),
            };

            self.rt().DrawRoundedRectangle(
                &rect.into().0,
                brush.get_ptr(),
                stroke_width,
                stroke_style,
            );
        }
    }

    #[inline]
    fn fill_rounded_rectangle<R, B>(&mut self, rect: R, brush: &B)
    where
        R: Into<RoundedRect>,
        B: Brush,
    {
        unsafe {
            self.rt()
                .FillRoundedRectangle(&rect.into().0, brush.get_ptr());
        }
    }

    #[inline]
    fn draw_ellipse<E, B>(
        &mut self,
        ellipse: E,
        brush: &B,
        stroke_width: f32,
        stroke_style: Option<&StrokeStyle>,
    ) where
        E: Into<Ellipse>,
        B: Brush,
    {
        unsafe {
            let stroke_style = match stroke_style {
                Some(s) => s.get_raw() as *mut _,
                None => ptr::null_mut(),
            };

            self.rt().DrawEllipse(
                &ellipse.into().0,
                brush.get_ptr(),
                stroke_width,
                stroke_style,
            );
        }
    }

    #[inline]
    fn fill_ellipse<E, B>(&mut self, ellipse: E, brush: &B)
    where
        E: Into<Ellipse>,
        B: Brush,
    {
        unsafe {
            self.rt().FillEllipse(&ellipse.into().0, brush.get_ptr());
        }
    }

    #[inline]
    fn draw_geometry<G, B>(
        &mut self,
        geometry: &G,
        brush: &B,
        stroke_width: f32,
        stroke_style: Option<&StrokeStyle>,
    ) where
        G: Geometry,
        B: Brush,
    {
        unsafe {
            let stroke_style = match stroke_style {
                Some(s) => s.get_raw() as *mut _,
                None => ptr::null_mut(),
            };

            self.rt().DrawGeometry(
                geometry.get_ptr(),
                brush.get_ptr(),
                stroke_width,
                stroke_style,
            );
        }
    }

    #[inline]
    fn fill_geometry<G, B>(&mut self, geometry: &G, brush: &B)
    where
        G: Geometry,
        B: Brush,
    {
        unsafe {
            self.rt()
                .FillGeometry(geometry.get_ptr(), brush.get_ptr(), ptr::null_mut());
        }
    }

    #[inline]
    fn fill_geometry_with_opacity<G, B, OB>(&mut self, geometry: &G, brush: &B, opacity_brush: &OB)
    where
        G: Geometry,
        B: Brush,
        OB: Brush,
    {
        unsafe {
            self.rt()
                .FillGeometry(geometry.get_ptr(), brush.get_ptr(), opacity_brush.get_ptr());
        }
    }

    #[inline]
    fn draw_bitmap<R0, R1>(
        &mut self,
        bitmap: &Bitmap,
        dest_rect: R0,
        opacity: f32,
        interpolation: BitmapInterpolationMode,
        src_rect: R1,
    ) where
        R0: Into<RectF>,
        R1: Into<RectF>,
    {
        unsafe {
            self.rt().DrawBitmap(
                bitmap.get_raw(),
                &dest_rect.into().0,
                opacity,
                interpolation as u32,
                &src_rect.into().0,
            );
        }
    }

    #[inline]
    fn draw_text<B, R>(
        &mut self,
        text: &str,
        format: &TextFormat,
        layout_rect: R,
        foreground_brush: &B,
        options: DrawTextOptions,
    ) where
        R: Into<RectF>,
        B: Brush,
    {
        let text = text.to_wide_null();

        unsafe {
            let format = format.get_raw();
            self.rt().DrawText(
                text.as_ptr(),
                text.len() as u32,
                format,
                &layout_rect.into().0,
                foreground_brush.get_ptr(),
                options.0,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    #[inline]
    fn draw_text_layout<P, B>(
        &mut self,
        origin: P,
        layout: &TextLayout,
        brush: &B,
        options: DrawTextOptions,
    ) where
        P: Into<Point2F>,
        B: Brush,
    {
        unsafe {
            let layout = layout.get_raw();
            self.rt()
                .DrawTextLayout(origin.into().0, layout, brush.get_ptr(), options.0);
        }
    }

    #[inline]
    fn set_transform(&mut self, transform: &Matrix3x2F) {
        unsafe { self.rt().SetTransform(&transform.0) }
    }

    #[inline]
    fn get_transform(&self) -> Matrix3x2F {
        unsafe {
            let mut mat: Matrix3x2F = mem::uninitialized();
            self.rt().GetTransform(&mut mat.0);
            mat
        }
    }

    #[inline]
    fn set_antialias_mode(&mut self, mode: AntialiasMode) {
        unsafe { self.rt().SetAntialiasMode(mode as u32) };
    }

    #[inline]
    fn get_antialias_mode(&mut self) -> UncheckedEnum<AntialiasMode> {
        unsafe { self.rt().GetAntialiasMode().into() }
    }

    #[inline]
    fn set_dpi(&mut self, dpi_x: f32, dpi_y: f32) {
        unsafe { self.rt().SetDpi(dpi_x, dpi_y) }
    }

    #[inline]
    fn get_dpi(&mut self) -> (f32, f32) {
        unsafe {
            let (mut x, mut y) = (0.0, 0.0);
            self.rt().GetDpi(&mut x, &mut y);
            (x, y)
        }
    }
}
