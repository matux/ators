use std::{cmp::Ordering, fmt, str::FromStr};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Addr(u64);

impl Addr {
    pub fn nil() -> Self {
        Self(0)
    }

    pub fn is_nil(&self) -> bool {
        self == 0
    }
}

impl Default for Addr {
    fn default() -> Self {
        Self::nil()
    }
}

impl From<u64> for Addr {
    fn from(addr: u64) -> Self {
        Self(addr)
    }
}

impl FromStr for Addr {
    type Err = <u64 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>()
            .or_else(|_| u64::from_str_radix(s.trim_start_matches("0x"), 16))
            .map(Addr::from)
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("{:#018x}", self.0))
    }
}

impl fmt::Debug for Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl PartialEq<u64> for Addr {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialEq<u64> for &Addr {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Addr> for u64 {
    fn eq(&self, other: &Addr) -> bool {
        other.0 == *self
    }
}

impl PartialOrd<u64> for Addr {
    fn partial_cmp(&self, other: &u64) -> Option<Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialOrd<u64> for &Addr {
    fn partial_cmp(&self, other: &u64) -> Option<Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialOrd<Addr> for u64 {
    fn partial_cmp(&self, other: &Addr) -> Option<Ordering> {
        other.0.partial_cmp(self)
    }
}

macro_rules! binops {
    ($Out:tt $i:ident $e:expr => $(($Lhs:ty, $Rhs:ty))*) => { $(
        impl std::ops::Add<$Rhs> for $Lhs {
            type Output = $Out;

            fn add(self, $i: $Rhs) -> $Out {
                $Out(self.0 + $e)
            }
        }

        impl std::ops::Sub<$Rhs> for $Lhs {
            type Output = $Out;

            fn sub(self, $i: $Rhs) -> $Out {
                $Out(self.0 - $e)
            }
        }
    )* }
}

macro_rules! add_sub_impl {
    ($tt:tt) => { binops!($tt rhs rhs.0 => ($tt, $tt)($tt, &$tt)(&$tt, $tt)(&$tt, &$tt)); };
    ($tl:tt $tr:ty) => { binops!($tl rhs rhs => ($tl, $tr)($tl, &$tr)(&$tl, $tr)(&$tl, &$tr)); }
}

add_sub_impl!(Addr);
add_sub_impl!(Addr u64);
