//! `no_std` linear algebra routines

use num_traits::{One, Zero, one, zero};

#[derive(Copy, Clone, Debug, Default)]
pub struct Vec3<T>(pub [T; 3]);

pub type Vec3f = Vec3<f32>;

impl<T> Vec3<T> {
    pub fn new(a: T, b: T, c: T) -> Self {
        Vec3([a, b, c])
    }

    pub fn project(self) -> Vec2<T>
        where T: Clone + core::ops::Div<Output = T>
    {
        let [a, b, c] = self.0;
        Vec2([a / c.clone(), b / c])
    }
}

trait Vector {
    type Element;
    fn dot(self, other: Self) -> Self::Element;
}

impl<T> core::iter::Sum for Vec3<T>
    where T: Default + core::ops::AddAssign
{
    fn sum<I>(iter: I) -> Self
        where I: core::iter::Iterator<Item = Self>,
    {
        let mut sum = Vec3::default();
        for Vec3([a, b, c]) in iter {
            sum.0[0] += a;
            sum.0[1] += b;
            sum.0[2] += c;
        }
        sum
    }
}

impl<T> Vector for Vec3<T>
where T: core::ops::Mul<Output = T> + core::ops::Add<Output = T>
{
    type Element = T;
    fn dot(self, other: Self) -> Self::Element
    {
        let [a0, b0, c0] = self.0;
        let [a1, b1, c1] = other.0;
        a0 * a1 + b0 * b1 + c0 * c1
    }
}


#[derive(Copy, Clone, Debug, Default)]
pub struct Vec2<T>(pub [T; 2]);

pub type Vec2f = Vec2<f32>;

impl<T> Vec2<T> {
    pub fn new(a: T, b: T) -> Self {
        Vec2([a, b])
    }

    pub fn augment(self) -> Vec3<T>
        where T: One
    {
        let [a, b] = self.0;
        Vec3([a, b, one()])
    }
}

impl<T> core::ops::Add for Vec2<T>
where T: core::ops::Add<Output = T>
{
    type Output = Vec2<T>;

    fn add(self, other: Self) -> Self {
        let [a0, b0] = self.0;
        let [a1, b1] = other.0;
        Vec2([a0 + a1, b0 + b1])
    }
}

impl<T> core::ops::Sub for Vec2<T>
where T: core::ops::Sub<Output = T>
{
    type Output = Vec2<T>;

    fn sub(self, other: Self) -> Self {
        let [a0, b0] = self.0;
        let [a1, b1] = other.0;
        Vec2([a0 - a1, b0 - b1])
    }
}

impl<T> core::ops::Mul<T> for Vec2<T>
where T: core::ops::Mul<Output = T> + Clone
{
    type Output = Vec2<T>;

    fn mul(self, other: T) -> Self {
        let [a0, b0] = self.0;
        Vec2([a0 * other.clone(), b0 * other])
    }
}

impl<T> Vector for Vec2<T>
where T: core::ops::Mul<Output = T> + core::ops::Add<Output = T>
{
    type Element = T;
    fn dot(self, other: Self) -> Self::Element
    {
        let [a0, b0] = self.0;
        let [a1, b1] = other.0;
        a0 * a1 + b0 * b1
    }
}


#[derive(Copy, Clone, Debug)]
pub struct Mat3<T>(pub [Vec3<T>; 3]);

pub type Mat3f = Mat3<f32>;

impl<T> core::ops::Mul<Vec3<T>> for Mat3<T>
where T: core::ops::Mul<Output = T> + core::ops::Add<Output = T>,
      Vec3<T>: Clone,
{
    type Output = Vec3<T>;
    fn mul(self, v: Vec3<T>) -> Self::Output {
        let [m0, m1, m2] = self.0;
        Vec3([
             v.clone().dot(m0),
             v.clone().dot(m1),
             v.dot(m2),
        ])
    }
}

impl<T> core::ops::Mul<Mat3<T>> for Mat3<T>
where T: core::ops::Mul<Output = T> + core::ops::Add<Output = T>,
      Vec3<T>: Clone,
{
    type Output = Mat3<T>;
    fn mul(self, v: Mat3<T>) -> Self::Output {
        let [b0, b1, b2] = self.0;
        let [a0, a1, a2] = v.0;
        Mat3([
             Vec3([
                  a0.clone().dot(b0.clone()),
                  a1.clone().dot(b0.clone()),
                  a2.clone().dot(b0)
             ]),
             Vec3([
                  a0.clone().dot(b1.clone()),
                  a1.clone().dot(b1.clone()),
                  a2.clone().dot(b1),
             ]),
             Vec3([
                  a0.dot(b2.clone()),
                  a1.dot(b2.clone()),
                  a2.dot(b2),
             ]),
        ])
    }
}

impl<T> Mat3<T> {
    pub fn identity() -> Self
        where T: One + Zero
    {
        Mat3([
             Vec3([one(), zero(), zero()]),
             Vec3([zero(), one(), zero()]),
             Vec3([zero(), zero(), one()]),
        ])
    }

    pub fn scale(x: T, y: T) -> Self
        where T: One + Zero
    {
        Mat3([
             Vec3([x, zero(), zero()]),
             Vec3([zero(), y, zero()]),
             Vec3([zero(), zero(), one()]),
        ])
    }

    pub fn rotate(cos: T, sin: T) -> Self
        where T: One + Zero + core::ops::Neg<Output = T> + Clone
    {
        Mat3([
             Vec3([cos.clone(), sin.clone(), zero()]),
             Vec3([-sin, cos, zero()]),
             Vec3([zero(), zero(), one()]),
        ])
    }

    pub fn translate(x: T, y: T) -> Self
        where T: One + Zero
    {
        Mat3([
             Vec3([one(), zero(), zero()]),
             Vec3([zero(), one(), zero()]),
             Vec3([x, y, one()]),
        ])
    }
}


pub fn lerp<T, R>(a: T, b: T, amt: R) -> T
    where T: core::ops::Sub<Output = T> + Clone,
          T: core::ops::Mul<R, Output = T> + core::ops::Add<Output = T>
{
    let delta = b.clone() - a.clone();
    a + (delta * amt)
}
