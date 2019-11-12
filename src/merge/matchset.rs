pub struct MatchSet<T>(MatchSetInner<T>);

impl<T> Default for MatchSet<T> {
    fn default() -> Self {
        MatchSet(MatchSetInner::Zero)
    }
}

impl<T> IntoIterator for MatchSet<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;
    fn into_iter(self) -> IntoIter<T> {
        IntoIter(match self.0 {
            MatchSetInner::Zero => IntoIterInner::Zero,
            MatchSetInner::One(v) => IntoIterInner::One(v),
            MatchSetInner::Many(vs) => IntoIterInner::Many(vs.into_iter()),
        })
    }
}

impl<T> MatchSet<T>
where
    T: PartialEq,
{
    pub fn insert(&mut self, v: T) {
        use MatchSetInner::*;
        let mut inner = Zero;
        std::mem::swap(&mut inner, &mut self.0);

        self.0 = match inner {
            Zero => One(v),
            One(existing) => {
                if existing == v {
                    One(existing)
                } else {
                    Many(vec![existing, v])
                }
            }
            Many(mut vs) => {
                if !vs.contains(&v) {
                    vs.push(v);
                }
                Many(vs)
            }
        };
    }
}

enum MatchSetInner<T> {
    Zero,
    One(T),
    Many(Vec<T>),
}

pub struct IntoIter<T>(IntoIterInner<T>);

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        use IntoIterInner::*;
        let mut inner = Zero;
        std::mem::swap(&mut inner, &mut self.0);
        match inner {
            Zero => {
                self.0 = Zero;
                None
            }
            One(v) => {
                self.0 = Zero;
                Some(v)
            }
            Many(mut i) => {
                let v = i.next();
                self.0 = Many(i);
                v
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        use IntoIterInner::*;
        match &self.0 {
            Zero => (0, Some(0)),
            One(_) => (1, Some(1)),
            Many(i) => i.size_hint(),
        }
    }
}

enum IntoIterInner<T> {
    Zero,
    One(T),
    Many(std::vec::IntoIter<T>),
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;
    #[test_case(vec![], vec![]; "empty input to empty output")]
    #[test_case(vec![1, 1, 1, 1], vec![1]; "deduping to single")]
    #[test_case(vec![1, 2, 1, 2], vec![1, 2]; "deduping to two")]
    #[test_case(vec![1, 2, 3, 4], vec![1, 2, 3, 4]; "same items")]
    fn add_dedupes(input: Vec<i8>, want: Vec<i8>) {
        let mut m = MatchSet::default();
        input.into_iter().for_each(|v| m.insert(v));
        let got: Vec<i8> = m.into_iter().collect();
        assert_eq!(got, want);
    }
}
