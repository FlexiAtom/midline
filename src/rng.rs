//! 确定性 PRNG（splitmix64 播种 + xoshiro128++ 推进）。
//! 骨架不引入依赖；同种子同结果，覆盖"每日挑战本地种子"需求。

pub struct Rng {
    s: [u32; 4],
}

fn splitmix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn seeded(seed: u64) -> Self {
        let mut x = seed;
        let mut s = [0u32; 4];
        for slot in s.iter_mut() {
            let v = splitmix64(&mut x);
            *slot = ((v >> 32) as u32) ^ (v as u32);
        }
        if s == [0; 4] {
            s[0] = 0x1234_5678;
        }
        Rng { s }
    }

    fn next_u32(&mut self) -> u32 {
        let result = (self.s[0].wrapping_add(self.s[3])).rotate_left(7).wrapping_add(self.s[0]);
        let t = self.s[1] << 9;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(11);
        result
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u32() as usize) % n
    }

    pub fn chance(&mut self, num: u32, den: u32) -> bool {
        den > 0 && self.next_u32() % den < num
    }

    /// 就地洗牌（Fisher-Yates）
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_same_seed() {
        let mut a = Rng::seeded(42);
        let mut b = Rng::seeded(42);
        let ra: Vec<usize> = (0..50).map(|_| a.below(97)).collect();
        let rb: Vec<usize> = (0..50).map(|_| b.below(97)).collect();
        assert_eq!(ra, rb);
    }

    #[test]
    fn different_seed_differs() {
        let mut a = Rng::seeded(1);
        let mut b = Rng::seeded(2);
        let ra: Vec<usize> = (0..20).map(|_| a.below(1000)).collect();
        let rb: Vec<usize> = (0..20).map(|_| b.below(1000)).collect();
        assert_ne!(ra, rb);
    }
}
