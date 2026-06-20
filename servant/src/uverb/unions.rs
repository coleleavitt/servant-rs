/// A two-arm response union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Union2<A, B> {
    /// First arm.
    V0(A),
    /// Second arm.
    V1(B),
}

/// A three-arm response union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Union3<A, B, C> {
    /// First arm.
    V0(A),
    /// Second arm.
    V1(B),
    /// Third arm.
    V2(C),
}

/// A four-arm response union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Union4<A, B, C, D> {
    /// First arm.
    V0(A),
    /// Second arm.
    V1(B),
    /// Third arm.
    V2(C),
    /// Fourth arm.
    V3(D),
}
