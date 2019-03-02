//! `no_std` linear algebra routines

use num_traits::{one, zero, One, Zero};

pub trait Vector: Sized + core::ops::Add<Self, Output = Self> + Zero {
    type Element: Element;

    fn dot(self, other: Self) -> Self::Element;
}

pub trait Element:
    Sized + core::ops::Mul<Output = Self> + core::ops::Add<Output = Self> + Zero
{
}

pub trait Project: Vector
where
    Self::Element: core::ops::Div<Output = Self::Element>,
{
    type Project: Vector<Element = Self::Element>;

    fn project(self) -> Self::Project;
}

pub trait Augment: Vector
where
    Self::Element: One + core::ops::Div<Output = Self::Element>,
{
    type Augment: Project<Element = Self::Element>;

    fn augment(self) -> Self::Augment;
}

impl<T> Element for T where
    T: Sized
        + core::ops::Mul<Output = Self>
        + core::ops::Add<Output = Self>
        + Zero
{
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Vec4<T>(pub T, pub T, pub T, pub T);

pub type Vec4f = Vec4<f32>;

impl<T> Project for Vec4<T>
where
    T: Element + core::ops::Div<Output = T> + Clone,
{
    type Project = Vec3<T>;

    fn project(self) -> Vec3<T> {
        Vec3(
            self.0 / self.3.clone(),
            self.1 / self.3.clone(),
            self.2 / self.3,
        )
    }
}

impl<T> Vector for Vec4<T>
where
    T: core::ops::Mul<Output = T> + core::ops::Add<Output = T> + Zero,
{
    type Element = T;
    fn dot(self, other: Self) -> Self::Element {
        self.0 * other.0
            + self.1 * other.1
            + self.2 * other.2
            + self.3 * other.3
    }
}

impl<T> From<(T, T, T, T)> for Vec4<T> {
    fn from(q: (T, T, T, T)) -> Self {
        Vec4(q.0, q.1, q.2, q.3)
    }
}

impl<T> core::ops::Add for Vec4<T>
where
    T: core::ops::Add<Output = T>,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Vec4(
            self.0 + other.0,
            self.1 + other.1,
            self.2 + other.2,
            self.3 + other.3,
        )
    }
}

impl<T> Zero for Vec4<T>
where
    T: Zero,
{
    fn zero() -> Self {
        Vec4(zero(), zero(), zero(), zero())
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero()
            && self.1.is_zero()
            && self.2.is_zero()
            && self.3.is_zero()
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Vec3<T>(pub T, pub T, pub T);

pub type Vec3f = Vec3<f32>;

impl<T> Project for Vec3<T>
where
    T: Element + Clone + core::ops::Div<Output = T>,
{
    type Project = Vec2<T>;

    fn project(self) -> Vec2<T> {
        Vec2(self.0 / self.2.clone(), self.1 / self.2)
    }
}

impl<T> Augment for Vec3<T>
where
    T: Element + Clone + One + core::ops::Div<Output = T>,
{
    type Augment = Vec4<T>;

    fn augment(self) -> Vec4<T> {
        Vec4(self.0, self.1, self.2, one())
    }
}

impl<T> Vector for Vec3<T>
where
    T: core::ops::Mul<Output = T> + core::ops::Add<Output = T> + Zero,
{
    type Element = T;
    fn dot(self, other: Self) -> Self::Element {
        self.0 * other.0 + self.1 * other.1 + self.2 * other.2
    }
}

impl<T> From<(T, T, T)> for Vec3<T> {
    fn from(tri: (T, T, T)) -> Self {
        Vec3(tri.0, tri.1, tri.2)
    }
}

impl<T> core::ops::Add for Vec3<T>
where
    T: core::ops::Add<Output = T>,
{
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Vec3(self.0 + other.0, self.1 + other.1, self.2 + other.2)
    }
}

impl<T> Zero for Vec3<T>
where
    T: Zero,
{
    fn zero() -> Self {
        Vec3(zero(), zero(), zero())
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero() && self.1.is_zero() && self.2.is_zero()
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Vec2<T>(pub T, pub T);

pub type Vec2f = Vec2<f32>;
pub type Vec2i = Vec2<i32>;

impl<T> Vec2<T> {
    pub fn augment(self) -> Vec3<T>
    where
        T: One,
    {
        Vec3(self.0, self.1, one())
    }
}

impl<T> core::ops::Add for Vec2<T>
where
    T: core::ops::Add<Output = T>,
{
    type Output = Vec2<T>;

    fn add(self, other: Self) -> Self {
        Vec2(self.0 + other.0, self.1 + other.1)
    }
}

impl<T> core::ops::Sub for Vec2<T>
where
    T: core::ops::Sub<Output = T>,
{
    type Output = Vec2<T>;

    fn sub(self, other: Self) -> Self {
        Vec2(self.0 - other.0, self.1 - other.1)
    }
}

impl<T> core::ops::Mul<T> for Vec2<T>
where
    T: core::ops::Mul<Output = T> + Clone,
{
    type Output = Vec2<T>;

    fn mul(self, other: T) -> Self {
        Vec2(self.0 * other.clone(), self.1 * other)
    }
}

impl<T> Vector for Vec2<T>
where
    T: core::ops::Mul<Output = T> + core::ops::Add<Output = T> + Zero,
{
    type Element = T;
    fn dot(self, other: Self) -> Self::Element {
        self.0 * other.0 + self.1 * other.1
    }
}

impl<T> From<(T, T)> for Vec2<T> {
    fn from(pair: (T, T)) -> Self {
        Vec2(pair.0, pair.1)
    }
}

impl<T> Zero for Vec2<T>
where
    T: Zero,
{
    fn zero() -> Self {
        Vec2(zero(), zero())
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero() && self.1.is_zero()
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Mat4<T>(pub Vec4<T>, pub Vec4<T>, pub Vec4<T>, pub Vec4<T>);

pub type Mat4f = Mat4<f32>;

impl<T> core::ops::Mul<Vec4<T>> for Mat4<T>
where
    T: Element,
    Vec4<T>: Clone + Vector<Element = T>,
{
    type Output = Vec4<T>;
    fn mul(self, v: Vec4<T>) -> Self::Output {
        Vec4(
            self.0.dot(v.clone()),
            self.1.dot(v.clone()),
            self.2.dot(v.clone()),
            self.3.dot(v),
        )
    }
}

impl<T> core::ops::Mul<Mat4<T>> for Mat4<T>
where
    T: Element,
    Vec4<T>: Clone + Vector<Element = T>,
{
    type Output = Mat4<T>;
    fn mul(self, v: Mat4<T>) -> Self::Output {
        let v = v.transpose();
        Mat4(
            Vec4(
                v.0.clone().dot(self.0.clone()),
                v.1.clone().dot(self.0.clone()),
                v.2.clone().dot(self.0.clone()),
                v.3.clone().dot(self.0),
            ),
            Vec4(
                v.0.clone().dot(self.1.clone()),
                v.1.clone().dot(self.1.clone()),
                v.2.clone().dot(self.1.clone()),
                v.3.clone().dot(self.1),
            ),
            Vec4(
                v.0.clone().dot(self.2.clone()),
                v.1.clone().dot(self.2.clone()),
                v.2.clone().dot(self.2.clone()),
                v.3.clone().dot(self.2),
            ),
            Vec4(
                v.0.dot(self.3.clone()),
                v.1.dot(self.3.clone()),
                v.2.dot(self.3.clone()),
                v.3.dot(self.3),
            ),
        )
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Mat3<T>(pub Vec3<T>, pub Vec3<T>, pub Vec3<T>);

pub type Mat3f = Mat3<f32>;

impl<T> core::ops::Mul<Vec3<T>> for Mat3<T>
where
    T: Element,
    Vec3<T>: Clone + Vector<Element = T>,
{
    type Output = Vec3<T>;
    fn mul(self, v: Vec3<T>) -> Self::Output {
        Vec3(v.clone().dot(self.0), v.clone().dot(self.1), v.dot(self.2))
    }
}

impl<T> core::ops::Mul<Mat3<T>> for Mat3<T>
where
    T: Element,
    Vec3<T>: Clone + Vector<Element = T>,
{
    type Output = Mat3<T>;
    fn mul(self, v: Mat3<T>) -> Self::Output {
        let v = v.transpose();
        Mat3(
            Vec3(
                v.0.clone().dot(self.0.clone()),
                v.1.clone().dot(self.0.clone()),
                v.2.clone().dot(self.0),
            ),
            Vec3(
                v.0.clone().dot(self.1.clone()),
                v.1.clone().dot(self.1.clone()),
                v.2.clone().dot(self.1),
            ),
            Vec3(
                v.0.dot(self.2.clone()),
                v.1.dot(self.2.clone()),
                v.2.dot(self.2),
            ),
        )
    }
}

impl<T> Mat3<T> {
    pub fn identity() -> Self
    where
        T: One + Zero,
    {
        Mat3(
            Vec3(one(), zero(), zero()),
            Vec3(zero(), one(), zero()),
            Vec3(zero(), zero(), one()),
        )
    }

    pub fn scale(x: T, y: T) -> Self
    where
        T: One + Zero,
    {
        Mat3(
            Vec3(x, zero(), zero()),
            Vec3(zero(), y, zero()),
            Vec3(zero(), zero(), one()),
        )
    }

    pub fn rotate_pre(sin: T, cos: T) -> Self
    where
        T: One + Zero + core::ops::Neg<Output = T> + Clone,
    {
        Mat3(
            Vec3(cos.clone(), sin.clone(), zero()),
            Vec3(-sin, cos, zero()),
            Vec3(zero(), zero(), one()),
        )
    }

    pub fn translate(x: T, y: T) -> Self
    where
        T: One + Zero,
    {
        Mat3(
            Vec3(one(), zero(), zero()),
            Vec3(zero(), one(), zero()),
            Vec3(x, y, one()),
        )
    }
}

impl Mat3<f32> {
    pub fn rotate(a: f32) -> Self {
        use libm::F32Ext;
        Self::rotate_pre(a.clone().sin(), a.cos())
    }
}

pub fn lerp<T, R>(a: T, b: T, amt: R) -> T
where
    T: core::ops::Sub<Output = T> + Clone,
    T: core::ops::Mul<R, Output = T> + core::ops::Add<Output = T>,
{
    let delta = b.clone() - a.clone();
    a + (delta * amt)
}

pub trait Matrix {
    type Element: Element;
    type Row: Vector<Element = Self::Element>;

    fn transpose(self) -> Self;
}

impl<T> Matrix for Mat3<T>
where
    T: Element,
{
    type Element = T;
    type Row = Vec3<T>;

    fn transpose(self) -> Self {
        Mat3(
            Vec3((self.0).0, (self.1).0, (self.2).0),
            Vec3((self.0).1, (self.1).1, (self.2).1),
            Vec3((self.0).2, (self.1).2, (self.2).2),
        )
    }
}

impl<T> Matrix for Mat4<T>
where
    T: Element,
{
    type Element = T;
    type Row = Vec4<T>;

    fn transpose(self) -> Self {
        Mat4(
            Vec4((self.0).0, (self.1).0, (self.2).0, (self.3).0),
            Vec4((self.0).1, (self.1).1, (self.2).1, (self.3).1),
            Vec4((self.0).2, (self.1).2, (self.2).2, (self.3).2),
            Vec4((self.0).3, (self.1).3, (self.2).3, (self.3).3),
        )
    }
}
