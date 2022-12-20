use std::num::NonZeroU8;

use crate::host::memory_manager::MemoryManager;
use crate::host::syscall_types::{SysCallReg, TypedPluginPtr};

use super::formatter::{
    FmtOptions, SyscallDataDisplay, SyscallPtr, SyscallPtrDisplay, TryFromSyscallReg,
};

/// Display the data using its `Display` implementation.
macro_rules! simple_display_impl {
    ($type:ty, $($types:ty),+) => {
        simple_display_impl!($type);
        simple_display_impl!($($types),+);
    };
    ($type:ty) => {
        impl SyscallDataDisplay for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self)
            }
        }
    };
}

/// Display the data using its `Debug` implementation.
macro_rules! simple_debug_impl {
    ($type:ty, $($types:ty),+) => {
        simple_debug_impl!($type);
        simple_debug_impl!($($types),+);
    };
    ($type:ty) => {
        impl SyscallDataDisplay for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:?}", self)
            }
        }
    };
}

/// Display the pointer and data. Accesses plugin memory. Can only be used for pod types.
macro_rules! simple_pointer_impl {
    ($type:ty, $($types:ty),+) => {
        simple_pointer_impl!($type);
        simple_pointer_impl!($($types),+);
    };
    ($type:ty) => {
        impl SyscallPtrDisplay for SyscallPtr<*const $type> {
            fn fmt(
                &self,
                f: &mut std::fmt::Formatter<'_>,
                options: FmtOptions,
                mem: &MemoryManager,
            ) -> std::fmt::Result {
                match (options, mem.memory_ref(TypedPluginPtr::new::<$type>(self.ptr, 1))) {
                    (FmtOptions::Standard, Ok(vals)) => write!(f, "{} ({:p})", &(*vals)[0], self.ptr),
                    (FmtOptions::Deterministic, Ok(_)) => write!(f, "<pointer>"),
                    // if we couldn't read the memory, just show the pointer instead
                    (FmtOptions::Standard, Err(_)) => write!(f, "{:p}", self.ptr),
                    (FmtOptions::Deterministic, Err(_)) => write!(f, "<pointer>"),
                }
            }
        }
    };
}

/// Display the pointer without dereferencing any plugin memory. Useful for types like *void, or
/// when we don't care about the data.
macro_rules! safe_pointer_impl {
    ($type:ty, $($types:ty),+) => {
        safe_pointer_impl!($type);
        safe_pointer_impl!($($types),+);
    };
    ($type:ty) => {
        impl SyscallPtrDisplay for SyscallPtr<*const $type> {
            fn fmt(
                &self,
                f: &mut std::fmt::Formatter<'_>,
                options: FmtOptions,
                _mem: &MemoryManager,
            ) -> std::fmt::Result {
                match options {
                    FmtOptions::Standard => write!(f, "{:p}", self.ptr),
                    FmtOptions::Deterministic => write!(f, "<pointer>"),
                }
            }
        }
    };
}

/// Display the array pointer and data. Accesses plugin memory. Can only be used for pod types.
macro_rules! simple_array_impl {
    ($type:ty, $($types:ty),+) => {
        simple_array_impl!($type);
        simple_array_impl!($($types),+);
    };
    ($type:ty) => {
        impl<const K: usize> SyscallPtrDisplay for SyscallPtr<[$type; K]> {
            fn fmt(
                &self,
                f: &mut std::fmt::Formatter<'_>,
                options: FmtOptions,
                mem: &MemoryManager,
            ) -> std::fmt::Result {
                match (options, mem.memory_ref(TypedPluginPtr::new::<$type>(self.ptr, K))) {
                    (FmtOptions::Standard, Ok(vals)) => write!(f, "{:?} ({:p})", &(*vals), self.ptr),
                    (FmtOptions::Deterministic, Ok(_)) => write!(f, "<pointer>"),
                    // if we couldn't read the memory, just show the pointer instead
                    (FmtOptions::Standard, Err(_)) => write!(f, "{:p}", self.ptr),
                    (FmtOptions::Deterministic, Err(_)) => write!(f, "<pointer>"),
                }
            }
        }
    };
}

// implement conversions from `SysCallReg`

impl TryFromSyscallReg for nix::fcntl::OFlag {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_bits(reg.into())
    }
}

impl TryFromSyscallReg for nix::sys::eventfd::EfdFlags {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_bits(reg.into())
    }
}

impl TryFromSyscallReg for nix::sys::socket::AddressFamily {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_i32(reg.into())
    }
}

impl TryFromSyscallReg for nix::sys::socket::MsgFlags {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_bits(reg.into())
    }
}

impl TryFromSyscallReg for nix::sys::stat::Mode {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_bits(reg.into())
    }
}

impl TryFromSyscallReg for nix::sys::mman::ProtFlags {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_bits(reg.into())
    }
}

impl TryFromSyscallReg for nix::sys::mman::MapFlags {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_bits(reg.into())
    }
}

impl TryFromSyscallReg for nix::sys::mman::MRemapFlags {
    fn try_from_reg(reg: SysCallReg) -> Option<Self> {
        Self::from_bits(reg.into())
    }
}

// implement display formatting

simple_display_impl!(i8, i16, i32, i64, isize);
simple_display_impl!(u8, u16, u32, u64, usize);

// skip *const i8 since we have a custom string format impl below
simple_pointer_impl!(i16, i32, i64, isize);
simple_pointer_impl!(u8, u16, u32, u64, usize);

simple_array_impl!(i8, i16, i32, i64, isize);
simple_array_impl!(u8, u16, u32, u64, usize);

safe_pointer_impl!(libc::c_void);
safe_pointer_impl!(libc::sockaddr);
safe_pointer_impl!(libc::sysinfo);

simple_debug_impl!(nix::fcntl::OFlag);
simple_debug_impl!(nix::sys::eventfd::EfdFlags);
simple_debug_impl!(nix::sys::socket::AddressFamily);
simple_debug_impl!(nix::sys::socket::MsgFlags);
simple_debug_impl!(nix::sys::stat::Mode);
simple_debug_impl!(nix::sys::mman::ProtFlags);
simple_debug_impl!(nix::sys::mman::MapFlags);
simple_debug_impl!(nix::sys::mman::MRemapFlags);

impl SyscallPtrDisplay for SyscallPtr<*const i8> {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
        options: FmtOptions,
        mem: &MemoryManager,
    ) -> std::fmt::Result {
        const DISPLAY_LEN: usize = 40;

        if options == FmtOptions::Deterministic {
            return write!(f, "<pointer>");
        }

        // read up to one extra character to check if it's a null byte
        let mem_ref =
            match mem.memory_ref_prefix(TypedPluginPtr::new::<u8>(self.ptr, DISPLAY_LEN + 1)) {
                Ok(x) => x,
                // the pointer didn't reference any valid memory
                Err(_) => return write!(f, "{:p}", self.ptr),
            };

        // to avoid printing too many escaped bytes, limit the number of non-graphic and non-ascii
        // characters
        let mut non_graphic_remaining = DISPLAY_LEN / 3;

        // mem_ref will reference up to DISPLAY_LEN+1 bytes
        let mut s: Vec<NonZeroU8> = mem_ref
            .iter()
            // get bytes until a null byte
            .map_while(|x| NonZeroU8::new(*x))
            // stop after a certain number of non-graphic characters
            .map_while(|x| {
                if !x.get().is_ascii_graphic() {
                    non_graphic_remaining = non_graphic_remaining.saturating_sub(1);
                }
                (non_graphic_remaining > 0).then_some(x)
            })
            .collect();

        let len = s.len();
        s.truncate(DISPLAY_LEN);
        let s: std::ffi::CString = s.into();

        #[allow(clippy::absurd_extreme_comparisons)]
        if len > DISPLAY_LEN || non_graphic_remaining <= 0 {
            write!(f, "{:?}...", s)
        } else {
            write!(f, "{:?}", s)
        }
    }
}
