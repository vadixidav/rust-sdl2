//! Audio Functions
//!
//! # Example
//! ```no_run
//! use sdl2::audio::{AudioCallback, AudioSpecDesired};
//! use std::time::Duration;
//!
//! struct SquareWave {
//!     phase_inc: f32,
//!     phase: f32,
//!     volume: f32
//! }
//!
//! impl AudioCallback for SquareWave {
//!     type Channel = f32;
//!
//!     fn callback(&mut self, out: &mut [f32]) {
//!         // Generate a square wave
//!         for x in out.iter_mut() {
//!             *x = match self.phase {
//!                 0.0...0.5 => self.volume,
//!                 _ => -self.volume
//!             };
//!             self.phase = (self.phase + self.phase_inc) % 1.0;
//!         }
//!     }
//! }
//!
//! let sdl_context = sdl2::init().unwrap();
//! let audio_subsystem = sdl_context.audio().unwrap();
//!
//! let desired_spec = AudioSpecDesired {
//!     freq: Some(44100),
//!     channels: Some(1),  // mono
//!     samples: None       // default sample size
//! };
//!
//! let device = audio_subsystem.open_playback(None, &desired_spec, |spec| {
//!     // initialize the audio callback
//!     SquareWave {
//!         phase_inc: 440.0 / spec.freq as f32,
//!         phase: 0.0,
//!         volume: 0.25
//!     }
//! }).unwrap();
//!
//! // Start playback
//! device.resume();
//!
//! // Play for 2 seconds
//! std::thread::sleep(Duration::from_millis(2000));
//! ```
use std::ffi::{CStr, CString};
use num::FromPrimitive;
use libc::{c_int, c_void, uint8_t, c_char};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::marker::PhantomData;
use std::mem;
use std::ptr;

use AudioSubsystem;
use get_error;
use rwops::RWops;

use sys::audio as ll;

impl AudioSubsystem {
    /// Opens a new audio device given the desired parameters and callback.
    #[inline]
    pub fn open_playback<CB, F>(&self, device: Option<&str>, spec: &AudioSpecDesired, get_callback: F) -> Result<AudioDevice <CB>, String>
    where CB: AudioCallback, F: FnOnce(AudioSpec) -> CB
    {
        AudioDevice::open_playback(self, device, spec, get_callback)
    }

    /// Opens a new audio device which uses queueing rather than older callback method.
    #[inline]
    pub fn open_queue<CB>(&self, device: Option<&str>, spec: &AudioSpecDesired) -> Result<AudioQueue, String>
    where CB: AudioCallback
    {
        AudioQueue::open_queue::<CB>(self, device, spec)
    }

    pub fn current_audio_driver(&self) -> &'static str {
        unsafe {
            let buf = ll::SDL_GetCurrentAudioDriver();
            assert!(!buf.is_null());

            CStr::from_ptr(buf as *const _).to_str().unwrap()
        }
    }

    pub fn num_audio_playback_devices(&self) -> Option<u32> {
        let result = unsafe { ll::SDL_GetNumAudioDevices(0) };
        if result < 0 {
            // SDL cannot retreive a list of audio devices. This is not necessarily an error (see the SDL2 docs).
            None
        } else {
            Some(result as u32)
        }
    }

    pub fn audio_playback_device_name(&self, index: u32) -> Result<String, String> {
        unsafe {
            let dev_name = ll::SDL_GetAudioDeviceName(index as c_int, 0);
            if dev_name.is_null() {
                Err(get_error())
            } else {
                let cstr = CStr::from_ptr(dev_name as *const _);
                Ok(cstr.to_str().unwrap().to_owned())
            }
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum AudioFormat {
    /// Unsigned 8-bit samples
    U8 = ll::AUDIO_U8 as isize,
    /// Signed 8-bit samples
    S8 = ll::AUDIO_S8 as isize,
    /// Unsigned 16-bit samples, little-endian
    U16LSB = ll::AUDIO_U16LSB as isize,
    /// Unsigned 16-bit samples, big-endian
    U16MSB = ll::AUDIO_U16MSB as isize,
    /// Signed 16-bit samples, little-endian
    S16LSB = ll::AUDIO_S16LSB as isize,
    /// Signed 16-bit samples, big-endian
    S16MSB = ll::AUDIO_S16MSB as isize,
    /// Signed 32-bit samples, little-endian
    S32LSB = ll::AUDIO_S32LSB as isize,
    /// Signed 32-bit samples, big-endian
    S32MSB = ll::AUDIO_S32MSB as isize,
    /// 32-bit floating point samples, little-endian
    F32LSB = ll::AUDIO_F32LSB as isize,
    /// 32-bit floating point samples, big-endian
    F32MSB = ll::AUDIO_F32MSB as isize
}

impl AudioFormat {
    fn from_ll(raw: ll::SDL_AudioFormat) -> Option<AudioFormat> {
        use self::AudioFormat::*;
        match raw {
            ll::AUDIO_U8 => Some(U8),
            ll::AUDIO_S8 => Some(S8),
            ll::AUDIO_U16LSB => Some(U16LSB),
            ll::AUDIO_U16MSB => Some(U16MSB),
            ll::AUDIO_S16LSB => Some(S16LSB),
            ll::AUDIO_S16MSB => Some(S16MSB),
            ll::AUDIO_S32LSB => Some(S32LSB),
            ll::AUDIO_S32MSB => Some(S32MSB),
            ll::AUDIO_F32LSB => Some(F32LSB),
            ll::AUDIO_F32MSB => Some(F32MSB),
            _ => None
        }
    }

    fn to_ll(self) -> ll::SDL_AudioFormat {
        self as ll::SDL_AudioFormat
    }
}

#[cfg(target_endian = "little")]
impl AudioFormat {
    /// Unsigned 16-bit samples, native endian
    #[inline] pub fn u16_sys() -> AudioFormat { AudioFormat::U16LSB }
    /// Signed 16-bit samples, native endian
    #[inline] pub fn s16_sys() -> AudioFormat { AudioFormat::S16LSB }
    /// Signed 32-bit samples, native endian
    #[inline] pub fn s32_sys() -> AudioFormat { AudioFormat::S32LSB }
    /// 32-bit floating point samples, native endian
    #[inline] pub fn f32_sys() -> AudioFormat { AudioFormat::F32LSB }
}

#[cfg(target_endian = "big")]
impl AudioFormat {
    /// Unsigned 16-bit samples, native endian
    #[inline] pub fn u16_sys() -> AudioFormat { AudioFormat::U16MSB }
    /// Signed 16-bit samples, native endian
    #[inline] pub fn s16_sys() -> AudioFormat { AudioFormat::S16MSB }
    /// Signed 32-bit samples, native endian
    #[inline] pub fn s32_sys() -> AudioFormat { AudioFormat::S32MSB }
    /// 32-bit floating point samples, native endian
    #[inline] pub fn f32_sys() -> AudioFormat { AudioFormat::F32MSB }
}

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum AudioStatus {
    Stopped = ll::SDL_AUDIO_STOPPED as isize,
    Playing = ll::SDL_AUDIO_PLAYING as isize,
    Paused  = ll::SDL_AUDIO_PAUSED  as isize,
}

impl FromPrimitive for AudioStatus {
    fn from_i64(n: i64) -> Option<AudioStatus> {
        use self::AudioStatus::*;

        Some( match n as ll::SDL_AudioStatus {
            ll::SDL_AUDIO_STOPPED => Stopped,
            ll::SDL_AUDIO_PLAYING => Playing,
            ll::SDL_AUDIO_PAUSED  => Paused,
            _                     => return None,
        })
    }

    fn from_u64(n: u64) -> Option<AudioStatus> { FromPrimitive::from_i64(n as i64) }
}

#[derive(Copy, Clone)]
pub struct DriverIterator {
    length: i32,
    index: i32
}

impl Iterator for DriverIterator {
    type Item = &'static str;

    #[inline]
    fn next(&mut self) -> Option<&'static str> {
        if self.index >= self.length {
            None
        } else {
            unsafe {
                let buf = ll::SDL_GetAudioDriver(self.index);
                assert!(!buf.is_null());
                self.index += 1;

                Some(CStr::from_ptr(buf as *const _).to_str().unwrap())
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let l = self.length as usize;
        (l, Some(l))
    }
}

impl ExactSizeIterator for DriverIterator { }

/// Gets an iterator of all audio drivers compiled into the SDL2 library.
#[inline]
pub fn drivers() -> DriverIterator {
    // This function is thread-safe and doesn't require the audio subsystem to be initialized.
    // The list of drivers are read-only and statically compiled into SDL2, varying by platform.

    // SDL_GetNumAudioDrivers can never return a negative value.
    DriverIterator {
        length: unsafe { ll::SDL_GetNumAudioDrivers() },
        index: 0
    }
}

pub struct AudioSpecWAV {
    pub freq: i32,
    pub format: AudioFormat,
    pub channels: u8,
    audio_buf: *mut u8,
    audio_len: u32
}

impl AudioSpecWAV {
    /// Loads a WAVE from the file path.
    pub fn load_wav<P: AsRef<Path>>(path: P) -> Result<AudioSpecWAV, String> {
        let mut file = try!(RWops::from_file(path, "rb"));
        AudioSpecWAV::load_wav_rw(&mut file)
    }

    /// Loads a WAVE from the data source.
    pub fn load_wav_rw(src: &mut RWops) -> Result<AudioSpecWAV, String> {
        use std::mem::uninitialized;
        use std::ptr::null_mut;

        let mut desired = unsafe { uninitialized::<ll::SDL_AudioSpec>() };
        let mut audio_buf: *mut u8 = null_mut();
        let mut audio_len: u32 = 0;
        unsafe {
            let ret = ll::SDL_LoadWAV_RW(src.raw(), 0, &mut desired, &mut audio_buf, &mut audio_len);
            if ret.is_null() {
                Err(get_error())
            } else {
                Ok(AudioSpecWAV {
                    freq: desired.freq,
                    format: AudioFormat::from_ll(desired.format).unwrap(),
                    channels: desired.channels,
                    audio_buf: audio_buf,
                    audio_len: audio_len
                })
            }
        }
    }

    pub fn buffer(&self) -> &[u8] {
        use std::slice::from_raw_parts;
        unsafe {
            let ptr = self.audio_buf as *const u8;
            let len = self.audio_len as usize;
            from_raw_parts(ptr, len)
        }
    }
}

impl Drop for AudioSpecWAV {
    fn drop(&mut self) {
        unsafe { ll::SDL_FreeWAV(self.audio_buf); }
    }
}

pub trait AudioCallback: Send
where Self::Channel: AudioFormatNum + 'static
{
    type Channel;

    fn callback(&mut self, &mut [Self::Channel]);
}

/// A phantom type for retreiving the SDL_AudioFormat of a given generic type.
/// All format types are returned as native-endian.
pub trait AudioFormatNum {
    fn audio_format() -> AudioFormat;
    fn zero() -> Self;
}

/// AUDIO_S8
impl AudioFormatNum for i8 {
    fn audio_format() -> AudioFormat { AudioFormat::S8 }
    fn zero() -> i8 { 0 }
}
/// AUDIO_U8
impl AudioFormatNum for u8 {
    fn audio_format() -> AudioFormat { AudioFormat::U8 }
    fn zero() -> u8 { 0 }
}
/// AUDIO_S16
impl AudioFormatNum for i16 {
    fn audio_format() -> AudioFormat { AudioFormat::s16_sys() }
    fn zero() -> i16 { 0 }
}
/// AUDIO_U16
impl AudioFormatNum for u16 {
    fn audio_format() -> AudioFormat { AudioFormat::u16_sys() }
    fn zero() -> u16 { 0 }
}
/// AUDIO_S32
impl AudioFormatNum for i32 {
    fn audio_format() -> AudioFormat { AudioFormat::s32_sys() }
    fn zero() -> i32 { 0 }
}
/// AUDIO_F32
impl AudioFormatNum for f32 {
    fn audio_format() -> AudioFormat { AudioFormat::f32_sys() }
    fn zero() -> f32 { 0.0 }
}

extern "C" fn audio_callback_marshall<CB: AudioCallback>
(userdata: *mut c_void, stream: *mut uint8_t, len: c_int) {
    use std::slice::from_raw_parts_mut;
    use std::mem::{size_of, transmute};
    unsafe {
        let mut cb_userdata: &mut CB = transmute(userdata);
        let buf: &mut [CB::Channel] = from_raw_parts_mut(
            stream as *mut CB::Channel,
            len as usize / size_of::<CB::Channel>()
        );

        cb_userdata.callback(buf);
    }
}

#[derive(Clone)]
pub struct AudioSpecDesired {
    /// DSP frequency (samples per second). Set to None for the device's fallback frequency.
    pub freq: Option<i32>,
    /// Number of separate audio channels. Set to None for the device's fallback number of channels.
    pub channels: Option<u8>,
    /// Audio buffer size in samples (power of 2). Set to None for the device's fallback sample size.
    pub samples: Option<u16>,
}

impl AudioSpecDesired {
    fn convert_to_ll<CB: AudioCallback>(freq: Option<i32>, channels: Option<u8>, samples: Option<u16>, userdata: Option<*mut CB>) -> ll::SDL_AudioSpec {
        use std::mem::transmute;

        if let Some(freq) = freq { assert!(freq > 0); }
        if let Some(channels) = channels { assert!(channels > 0); }
        if let Some(samples) = samples { assert!(samples > 0); }

        // A value of 0 means "fallback" or "default".

        

        unsafe {
            ll::SDL_AudioSpec {
                freq: freq.unwrap_or(0),
                format: <CB::Channel as AudioFormatNum>::audio_format().to_ll(),
                channels: channels.unwrap_or(0),
                silence: 0,
                samples: samples.unwrap_or(0),
                padding: 0,
                size: 0,
                callback: if userdata.is_some() {
                    Some(audio_callback_marshall::<CB>
                        as extern "C" fn
                            (arg1: *mut c_void,
                             arg2: *mut uint8_t,
                             arg3: c_int))
                }
                else {
                    None
                },
                userdata: if userdata.is_some() {
                    transmute(userdata.expect("userdata error in AudioSpecDesired::convert_to_ll"))
                }
                else {
                    0 as *mut c_void
                }
            }
        }
    }
}

#[allow(missing_copy_implementations)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct AudioSpec {
    pub freq: i32,
    pub format: AudioFormat,
    pub channels: u8,
    pub silence: u8,
    pub samples: u16,
    pub size: u32
}

impl AudioSpec {
    fn convert_from_ll(spec: ll::SDL_AudioSpec) -> AudioSpec {
        AudioSpec {
            freq: spec.freq,
            format: AudioFormat::from_ll(spec.format).unwrap(),
            channels: spec.channels,
            silence: spec.silence,
            samples: spec.samples,
            size: spec.size
        }
    }
}

enum AudioDeviceID {
    PlaybackDevice(ll::SDL_AudioDeviceID)
}

impl AudioDeviceID {
    fn id(&self) -> ll::SDL_AudioDeviceID {
        match self {
            &AudioDeviceID::PlaybackDevice(id)  => id
        }
    }
}

impl Drop for AudioDeviceID {
    fn drop(&mut self) {
        //! Shut down audio processing and close the audio device.
        unsafe { ll::SDL_CloseAudioDevice(self.id()) }
    }
}

/// Wraps SDL_AudioDeviceID and owns the callback data used by the audio device.
pub struct AudioQueue {
    subsystem: AudioSubsystem,
    device_id: AudioDeviceID,
}

impl AudioQueue {
    /// Opens a new audio device given the desired parameters and callback.
    pub fn open_queue<CB> (a: &AudioSubsystem, device: Option<&str>, spec: &AudioSpecDesired) -> Result<AudioQueue, String> where CB: AudioCallback {
        let desired = AudioSpecDesired::convert_to_ll::<CB>(spec.freq, spec.channels, spec.samples, None);

        let mut obtained = unsafe { mem::uninitialized::<ll::SDL_AudioSpec>() };
        unsafe {
            let device = match device {
                Some(device) => Some(CString::new(device).unwrap()),
                None => None
            };
            let device_ptr = device.map_or(ptr::null(), |s| s.as_ptr());

            let iscapture_flag = 0;
            let device_id = ll::SDL_OpenAudioDevice(
                device_ptr as *const c_char, iscapture_flag, &desired, 
                &mut obtained, 0
            );
            match device_id {
                0 => {
                    Err(get_error())
                },
                id => {
                    let device_id = AudioDeviceID::PlaybackDevice(id);

                    Ok(AudioQueue {
                        subsystem: a.clone(),
                        device_id: device_id,
                    })
                }
            }
        }
    }

    #[inline]
    pub fn subsystem(&self) -> &AudioSubsystem { &self.subsystem }

    pub fn status(&self) -> AudioStatus {
        unsafe {
            let status = ll::SDL_GetAudioDeviceStatus(self.device_id.id());
            FromPrimitive::from_i32(status as i32).unwrap()
        }
    }

    /// Pauses playback of the audio device.
    pub fn pause(&self) {
        unsafe { ll::SDL_PauseAudioDevice(self.device_id.id(), 1) }
    }

    /// Starts playback of the audio device.
    pub fn resume(&self) {
        unsafe { ll::SDL_PauseAudioDevice(self.device_id.id(), 0) }
    }

    pub fn queue<Channel>(&self, data: &[Channel]) -> bool {
        let result = unsafe {ll::SDL_QueueAudio(self.device_id.id(), data.as_ptr() as *const c_void, data.len() as u32)};
        result == 0
    }
}

/// Wraps SDL_AudioDeviceID and owns the callback data used by the audio device.
pub struct AudioDevice<CB: AudioCallback> {
    subsystem: AudioSubsystem,
    device_id: AudioDeviceID,
    /// Store the callback to keep it alive for the entire duration of `AudioDevice`.
    userdata: Box<CB>
}

impl<CB: AudioCallback> AudioDevice<CB> {
    /// Opens a new audio device given the desired parameters and callback.
    pub fn open_playback<F>(a: &AudioSubsystem, device: Option<&str>, spec: &AudioSpecDesired, get_callback: F) -> Result<AudioDevice <CB>, String>
    where F: FnOnce(AudioSpec) -> CB
    {

        // SDL_OpenAudioDevice needs a userdata pointer, but we can't initialize the
        // callback without the obtained AudioSpec.
        // Create an uninitialized box that will be initialized after SDL_OpenAudioDevice.
        let userdata: *mut CB = unsafe {
            let b: Box<CB> = Box::new(mem::uninitialized());
            mem::transmute(b)
        };
        let desired = AudioSpecDesired::convert_to_ll(spec.freq, spec.channels, spec.samples, Some(userdata));

        let mut obtained = unsafe { mem::uninitialized::<ll::SDL_AudioSpec>() };
        unsafe {
            let device = match device {
                Some(device) => Some(CString::new(device).unwrap()),
                None => None
            };
            let device_ptr = device.map_or(ptr::null(), |s| s.as_ptr());

            let iscapture_flag = 0;
            let device_id = ll::SDL_OpenAudioDevice(
                device_ptr as *const c_char, iscapture_flag, &desired, 
                &mut obtained, 0
            );
            match device_id {
                0 => {
                    Err(get_error())
                },
                id => {
                    let device_id = AudioDeviceID::PlaybackDevice(id);
                    let spec = AudioSpec::convert_from_ll(obtained);
                    let mut userdata: Box<CB> = mem::transmute(userdata);

                    let garbage = mem::replace(&mut userdata as &mut CB, get_callback(spec));
                    mem::forget(garbage);

                    Ok(AudioDevice {
                        subsystem: a.clone(),
                        device_id: device_id,
                        userdata: userdata
                    })
                }
            }
        }
    }

    #[inline]
    pub fn subsystem(&self) -> &AudioSubsystem { &self.subsystem }

    pub fn status(&self) -> AudioStatus {
        unsafe {
            let status = ll::SDL_GetAudioDeviceStatus(self.device_id.id());
            FromPrimitive::from_i32(status as i32).unwrap()
        }
    }

    /// Pauses playback of the audio device.
    pub fn pause(&self) {
        unsafe { ll::SDL_PauseAudioDevice(self.device_id.id(), 1) }
    }

    /// Starts playback of the audio device.
    pub fn resume(&self) {
        unsafe { ll::SDL_PauseAudioDevice(self.device_id.id(), 0) }
    }

    /// Locks the audio device using `SDL_LockAudioDevice`.
    ///
    /// When the returned lock guard is dropped, `SDL_UnlockAudioDevice` is
    /// called.
    /// Use this method to read and mutate callback data.
    pub fn lock<'a>(&'a mut self) -> AudioDeviceLockGuard<'a, CB> {
        unsafe { ll::SDL_LockAudioDevice(self.device_id.id()) };
        AudioDeviceLockGuard {
            device:  self,
            _nosend: PhantomData
        }
    }

    /// Closes the audio device and saves the callback data from being dropped.
    ///
    /// Note that simply dropping `AudioDevice` will close the audio device,
    /// but the callback data will be dropped.
    pub fn close_and_get_callback(self) -> CB {
        drop(self.device_id);
        *self.userdata
    }
}

/// Similar to `std::sync::MutexGuard`, but for use with `AudioDevice::lock()`.
pub struct AudioDeviceLockGuard<'a, CB> where CB: AudioCallback, CB: 'a {
    device: &'a mut AudioDevice<CB>,
    _nosend: PhantomData<*mut ()>
}

impl<'a, CB: AudioCallback> Deref for AudioDeviceLockGuard<'a, CB> {
    type Target = CB;
    fn deref(&self) -> &CB { &self.device.userdata }
}

impl<'a, CB: AudioCallback> DerefMut for AudioDeviceLockGuard<'a, CB> {
    fn deref_mut(&mut self) -> &mut CB { &mut self.device.userdata }
}

impl<'a, CB: AudioCallback> Drop for AudioDeviceLockGuard<'a, CB> {
    fn drop(&mut self) {
        unsafe { ll::SDL_UnlockAudioDevice(self.device.device_id.id()) }
    }
}

#[derive(Copy, Clone)]
pub struct AudioCVT {
    raw: ll::SDL_AudioCVT
}

impl AudioCVT {
    pub fn new(src_format: AudioFormat, src_channels: u8, src_rate: i32,
               dst_format: AudioFormat, dst_channels: u8, dst_rate: i32) -> Result<AudioCVT, String>
    {
        use std::mem;
        unsafe {
            let mut raw: ll::SDL_AudioCVT = mem::uninitialized();
            let ret = ll::SDL_BuildAudioCVT(&mut raw,
                                            src_format.to_ll(), src_channels, src_rate as c_int,
                                            dst_format.to_ll(), dst_channels, dst_rate as c_int);
            if ret == 1 || ret == 0 {
                Ok(AudioCVT { raw: raw })
            } else {
                Err(get_error())
            }
        }
    }

    pub fn convert(&self, mut src: Vec<u8>) -> Vec<u8> {
        //! Convert audio data to a desired audio format.
        //!
        //! The `src` vector is adjusted to the capacity necessary to perform
        //! the conversion in place; then it is passed to the SDL library.
        //!
        //! Certain conversions may cause buffer overflows. See AngryLawyer/rust-sdl2 issue #270.
        use num::traits as num;
        unsafe {
            if self.raw.needed != 0 {
                let mut raw = self.raw;

                // calculate the size of the dst buffer
                raw.len = num::cast(src.len()).expect("Buffer length overflow");
                let dst_size = self.capacity(src.len());
                let needed = dst_size - src.len();
                src.reserve_exact(needed);

                // perform the conversion in place
                raw.buf = src.as_mut_ptr();
                let ret = ll::SDL_ConvertAudio(&mut raw);
                // There's no reason for SDL_ConvertAudio to fail.
                // The only time it can fail is if buf is NULL, which it never is.
                if ret != 0 { panic!(get_error()) }

                // return original buffer back to caller
                debug_assert!(raw.len_cvt > 0);
                debug_assert!(raw.len_cvt as usize <= src.capacity());

                src.set_len(raw.len_cvt as usize);
                src
            } else {
                // The buffer remains unmodified
                src
            }
        }
    }

    /// Checks if any conversion is needed. i.e. if the buffer that goes
    /// into `convert()` is unchanged from the result.
    pub fn is_conversion_needed(&self) -> bool { self.raw.needed != 0 }

    /// Gets the buffer capacity that can contain both the original and
    /// converted data.
    pub fn capacity(&self, src_len: usize) -> usize {
        src_len.checked_mul(self.raw.len_mult as usize).expect("Integer overflow")
    }
}


#[cfg(test)]
mod test {
    use super::{AudioCVT, AudioFormat};

    #[test]
    fn test_audio_cvt() {
        use std::iter::repeat;

        // 0,1,2,3, ...
        let buffer: Vec<u8> = (0..255).collect();

        // 0,0,1,1,2,2,3,3, ...
        let new_buffer_expected: Vec<u8> = (0..255).flat_map(|v| repeat(v).take(2)).collect();

        let cvt = AudioCVT::new(AudioFormat::U8, 1, 44100, AudioFormat::U8, 2, 44100).unwrap();
        assert!(cvt.is_conversion_needed());
        assert_eq!(cvt.capacity(255), 255*2);

        let new_buffer = cvt.convert(buffer);
        assert_eq!(new_buffer.len(), new_buffer_expected.len());
        assert_eq!(new_buffer, new_buffer_expected);
    }
}
