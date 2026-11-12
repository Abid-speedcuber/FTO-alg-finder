//! Safe prefetch hint; unsupported architectures retain ordinary reads.
#[inline]
pub fn prefetch(bytes: &[u8], index: usize) {
    #[cfg(target_arch = "x86_64")]
    if let Some(value) = bytes.get(index) {
        x86::prefetch(value);
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = (bytes, index);
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
mod x86 {
    #[inline]
    pub(super) fn prefetch(value: &u8) {
        // SAFETY: SSE is required by x86-64; value points into a live slice.
        // Prefetch is a hint and does not change observable memory.
        unsafe {
            std::arch::x86_64::_mm_prefetch(
                std::ptr::from_ref(value).cast(),
                std::arch::x86_64::_MM_HINT_T0,
            );
        }
    }
}
