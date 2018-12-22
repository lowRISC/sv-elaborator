use num::{BigUint, BigInt, bigint::Sign, One, Zero, ToPrimitive};
use std::fmt;

pub mod int;
use self::int::Int;

/// Represntation of Verilog's 4-state logic
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogicValue {
    Zero,
    One,
    Z,
    X,
}

impl fmt::Display for LogicValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            LogicValue::Zero => write!(f, "0"),
            LogicValue::One => write!(f, "1"),
            LogicValue::Z => write!(f, "z"),
            LogicValue::X => write!(f, "x"),
        }
    }
}

/// Represntation of Verilog's 4-state logic array
#[derive(Clone, PartialEq)]
pub struct LogicVec {
    pub signed: bool,
    value: Int,
    xz: Int,
}

impl LogicVec {

    /// Construct a 4-state logic array from 2-state logic.
    pub fn new_xz(width: usize, signed: bool, value: BigUint, xz: BigUint) -> LogicVec {
        LogicVec {
            signed,
            value: Int::new(width, value),
            xz: Int::new(width, xz),
        }
    }

    /// Construct a 4-state logic array from 2-state logic.
    pub fn new(width: usize, signed: bool, mut value: BigUint) -> LogicVec {
        if width < value.bits() {
            let mut val = BigUint::one();
            val <<= width;
            val -= 1 as u8;
            value &= val;
        }

        LogicVec::new_xz(
            width,
            signed,
            value,
            BigUint::zero(),
        )
    }

    /// Convert from BigInt
    pub fn from(width: usize, signed: bool, value: BigInt) -> LogicVec {
        let value = if let Sign::Minus = value.sign() {
            let mut abs = (-value).to_biguint().unwrap();
            let mut val = BigUint::one();
            val <<= width;
            val -= 1 as u8;
            // Invert all bits
            abs ^= val;
            abs + 1 as u8
        } else {
            value.to_biguint().unwrap()
        };
        Self::new(width, signed, value)
    }

    /// Fill a vector with a value
    pub fn fill(width: usize, signed: bool, value: LogicValue) -> LogicVec {
        let mut vec: Self = (&value).into();
        vec.signed = signed;
        vec.duplicate(width)
    }

    /// Get the width of this number
    pub fn width(&self) -> usize {
        self.value.width()
    }

    /// Check if this is signed
    pub fn signed(&self) -> bool {
        self.signed
    }

    /// Check if this is a 2-state logic
    pub fn is_two_state(&self) -> bool {
        self.xz.is_zero()
    }

    /// Convert to two state value. If there is a Z or X, `None` is returned.
    pub fn get_two_state(&self) -> Option<BigInt> {
        if self.is_two_state() {
            if self.signed {
                Some(self.value.clone().to_bigint_signed())
            } else {
                Some(self.value.clone().to_bigint_unsigned())
            }
        } else {
            None
        }
    }

    /// Force as two state value. X and Zs will be converted to 0.
    pub fn force_two_state(mut self) -> Self {
        self.value &= &!self.xz;
        self.xz = Int::zero(self.value.width());
        self
    }

    /// Perform sign extension or truncation
    pub fn sign_extend_or_trunc(&self, width: usize) -> Self {
        let mut value = self.value.clone();
        let mut xz = self.xz.clone();
        value.sign_extend_or_trunc(width);
        xz.sign_extend_or_trunc(width);

        Self {
            signed: self.signed,
            value,
            xz,
        }
    }

    /// Perform xz-extension or truncation
    pub fn xz_extend_or_trunc(&self, width: usize) -> Self {
        let mut value = self.value.clone();
        let mut xz = self.xz.clone();

        if self.xz.bit_at(self.value.width() - 1) {
            // If highest bit is XZ, do sign extension on value
            value.sign_extend_or_trunc(width);
            xz.one_extend_or_trunc(width);
        } else {
            value.zero_extend_or_trunc(width);
            xz.zero_extend_or_trunc(width);
        }

        Self {
            signed: self.signed,
            value,
            xz,
        }
    }

    /// Perform extension or truncation
    pub fn extend_or_trunc(&self, width: usize) -> Self {
        if self.signed {
            self.sign_extend_or_trunc(width)
        } else {
            self.xz_extend_or_trunc(width)
        }
    }

    pub fn duplicate(&self, count: usize) -> Self {
        Self {
            signed: self.signed,
            value: self.value.duplicate(count),
            xz: self.xz.duplicate(count),
        }
    }
}

//
// Arithmetic of LogicVec
//

impl LogicVec {
    pub fn l_shr(mut self, rhs: &Self) -> Self {
        // The rhs should always be unsigned.
        assert!(!rhs.signed);

        // If right hand side is not two-state, then this is a X.
        if !rhs.is_two_state() {
            return Self::fill(self.value.width(), self.signed, LogicValue::X);
        }

        if self.signed {
            self.value.sign_shr(&rhs.value);
            self.xz.sign_shr(&rhs.value);
        } else {
            self.value.zero_shr(&rhs.value);
            self.xz.zero_shr(&rhs.value);
        }

        self
    }
}

impl<'a> From<&'a LogicValue> for LogicVec {
    fn from(val: &'a LogicValue) -> LogicVec {
        let (xz, value) = match val {
            LogicValue::Zero => (BigUint::zero(), BigUint::zero()),
            LogicValue::One => (BigUint::zero(), BigUint::one()),
            LogicValue::Z => (BigUint::one(), BigUint::zero()),
            LogicValue::X => (BigUint::one(), BigUint::one()),
        };
        LogicVec::new_xz(
            1,
            false,
            value,
            xz,
        )
    }
}

impl fmt::Debug for LogicVec {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let width = self.width();
        let value = self.value.clone().to_bigint_unsigned();
        let xz = self.xz.clone().to_bigint_unsigned();
        write!(f, "{}'", width)?;
        if self.signed {
            write!(f, "s")?;
        }
        if self.is_two_state() {
            // We use a heuristics for display number. If the number is below 1024, display it as
            // decimal. Otherwise display as hex.
            if &value < &(1024 as u16).into() {
                write!(f, "d{}", value)
            } else {
                write!(f, "h{:X}", value)
            }
        } else {
            let mut str = format!("{:0width$b}", value, width=width).into_bytes();
            let xz = format!("{:0width$b}", xz, width=width).into_bytes();
            for i in 0..xz.len() {
                if xz[i] == b'1' {
                    str[i] = if str[i] == b'0' { b'z' } else { b'x' }
                }
            }
            write!(f, "b{}", String::from_utf8(str).unwrap())
        }
    }
}

/// Represntation of Verilog's 4-state logic array
#[derive(Clone, PartialEq)]
pub struct LogicNumber {
    pub sized: bool,
    pub value: LogicVec,
}

impl fmt::Display for LogicNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let width = self.value.width();
        let value = self.value.value.clone().to_bigint_unsigned();
        let xz = self.value.xz.clone().to_bigint_unsigned();

        // In this case we can simply print out a decimal
        if self.value.is_two_state() && !self.sized && self.value.signed {
            return write!(f, "{}", value)
        }
        if self.sized {
            write!(f, "{}", width)?;
        }
        write!(f, "'")?;
        if self.value.signed {
            write!(f, "s")?;
        }
        if xz.is_zero() {
            write!(f, "d{}", value)
        } else {
            let mut str = format!("{:0width$b}", value, width=width).into_bytes();
            let xz = format!("{:0width$b}", xz, width=width).into_bytes();
            for i in 0..xz.len() {
                if xz[i] == b'1' {
                    str[i] = if str[i] == b'0' { b'z' } else { b'x' }
                }
            }
            write!(f, "b{}", String::from_utf8(str).unwrap())
        }
    }
}

impl fmt::Debug for LogicNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        fmt::Display::fmt(self, f)
    }
}