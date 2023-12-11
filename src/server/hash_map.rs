use std::borrow::Borrow;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::mem;

fn calculate_hash<T: Hash>(value: &T) -> u64 {
    let mut s = DefaultHasher::new();

    value.hash(&mut s);
    s.finish()
}

#[derive(Debug)]
struct Entry<K, V> {
    key: K,
    value: V,
}

#[derive(Debug)]
struct HashMap<K, V> {
    data: Vec<Vec<Entry<K, V>>>,
    size: usize,
    mask: u64,
}

impl<K, V> HashMap<K, V> {
    fn new(size: usize) -> Self {
        assert!(size > 0 && ((size - 1) & size) == 0);

        let mut data = Vec::with_capacity(size);
        for _ in 0..size {
            data.push(Vec::new());
        }

        Self {
            data,
            mask: (size - 1) as u64,
            size: 0,
        }
    }

    fn len(&self) -> usize {
        self.size
    }

    fn insert(&mut self, key: K, value: V)
    where
        K: Hash + Eq,
    {
        let pos = (calculate_hash(&key) & self.mask) as usize;

        // NOTE(vincent): safe because we always initialize `data`
        let list = self.data.get_mut(pos).unwrap();

        // Try to update the value first
        for entry in list.iter_mut() {
            if entry.key == key {
                entry.value = value;
                return;
            }
        }

        // Otherwise insert it
        list.push(Entry { key, value });
        self.size += 1
    }

    fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let pos = (calculate_hash(&key) & self.mask) as usize;

        // NOTE(vincent): safe because we always initialize `data`
        let list = self.data.get(pos).unwrap();

        list.iter()
            .find(|entry| entry.key.borrow() == key)
            .map(|entry| &entry.value)
    }

    fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let pos = (calculate_hash(&key) & self.mask) as usize;

        // NOTE(vincent): safe because we always initialize `data`
        let list = self.data.get_mut(pos).unwrap();

        for (i, entry) in list.iter().enumerate() {
            if entry.key.borrow() == key {
                let entry = list.swap_remove(i);
                return Some(entry.value);
            }
        }

        None
    }
}

#[allow(dead_code)]
fn dump_hashmap<K: Hash + Eq + Debug, V: Eq + Debug>(name: &str, map: &HashMap<K, V>) {
    println!("map {}", name);

    for (i, list) in map.data.iter().enumerate() {
        println!("  bucket #{}", i);
        for entry in list.iter() {
            println!("    {:?}: {:?}", entry.key, entry.value);
        }
    }
}

#[allow(dead_code)]
fn dump_superhashmap<K: Hash + Eq + Debug, V: Eq + Debug>(map: &SuperHashMap<K, V>) {
    println!(
        "superhashmap: size={} buckets={}",
        map.map1.len() + map.map2.as_ref().map(|m| m.len()).unwrap_or_default(),
        map.map1.data.len() + map.map2.as_ref().map(|m| m.data.len()).unwrap_or_default(),
    );

    let dump = |name: &str, map: &HashMap<K, V>| {
        println!("    map {}", name);

        for (i, list) in map.data.iter().enumerate() {
            println!("         bucket #{}", i);
            for entry in list.iter() {
                println!("            {:?}: {:?}", entry.key, entry.value);
            }
        }
    };

    dump("map1", &map.map1);
    if let Some(ref m) = map.map2 {
        dump("map2", m);
    }
}

#[derive(Debug)]
pub struct SuperHashMap<K, V> {
    map1: HashMap<K, V>,
    map2: Option<HashMap<K, V>>,

    resizing_pos: usize,
}

impl<K, V> SuperHashMap<K, V>
where
    K: Hash + Eq,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            map1: HashMap::new(capacity),
            map2: None,
            resizing_pos: 0,
        }
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(value) = self.map1.get(key) {
            return Some(value);
        }

        if let Some(m) = &self.map2 {
            m.get(key)
        } else {
            None
        }
    }

    pub fn insert(&mut self, key: K, value: V)
    where
        K: Hash + Eq,
    {
        self.map1.insert(key, value);

        {
            let load_factor = self.map1.size / (self.map1.mask + 1) as usize;
            if load_factor > MAX_LOAD_FACTOR {
                self.start_resizing();
            }
        }

        self.help_resizing();
    }

    pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        if let Some(value) = self.map1.remove(key) {
            return Some(value);
        }

        self.map2.as_mut().map(|m| m.remove(key)).flatten()
    }

    fn start_resizing(&mut self) {
        let new_capacity = ((self.map1.mask + 1) * 2) as usize;

        let old_map1 = mem::replace(&mut self.map1, HashMap::new(new_capacity));
        self.map2 = Some(old_map1)
    }

    fn help_resizing(&mut self) {
        if let Some(m) = &mut self.map2 {
            // Move up to [`MAX_RESIZING_WORK`] items

            let mut work = 0;
            'outer: for list in &mut m.data[self.resizing_pos..] {
                while let Some(entry) = list.pop() {
                    self.map1.insert(entry.key, entry.value);
                    work += 1;

                    if work > MAX_RESIZING_WORK {
                        break 'outer;
                    }
                }

                self.resizing_pos += 1;
            }

            // If we moved every bucket in map2, remove it
            if self.resizing_pos >= m.data.len() {
                self.map2.take().map(|value| {
                    drop(value);
                    self.resizing_pos = 0;
                });
            }
        }
    }
}

const MAX_RESIZING_WORK: usize = 128;
const MAX_LOAD_FACTOR: usize = 8;

#[cfg(test)]
mod tests {
    use crate::hash_map::dump_superhashmap;

    use super::{HashMap, SuperHashMap};

    #[test]
    fn simple() {
        let mut table = HashMap::new(1);

        table.insert("foobar", "hallo");
        table.insert("barbaz", "hello");
        table.insert("bazqux", "salut");

        assert_eq!(table.get("foobar"), Some(&"hallo"));
        assert_eq!(table.get("barbaz"), Some(&"hello"));
        assert_eq!(table.get("bazqux"), Some(&"salut"));
    }

    #[test]
    fn insert_multiple_times() {
        let mut table = HashMap::new(1);

        table.insert("foobar", "hallo");
        table.insert("foobar", "hullo");
        table.insert("foobar", "hello");

        assert_eq!(table.get("foobar"), Some(&"hello"));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn super_hashmap_simple() {
        let mut map = SuperHashMap::new(1);

        static NB: usize = 100;

        for i in 0..NB {
            map.insert(format!("foo{}", i), i);
        }

        for i in 0..NB {
            let key = format!("foo{}", i);
            assert_eq!(map.get(&key), Some(&i));
        }

        // dump_superhashmap(&map);
    }

    #[test]
    fn super_hashmap_remove() {
        let mut map = SuperHashMap::new(1);

        map.insert("foobar", "barbaz");
        map.insert("hello", "world");

        dump_superhashmap(&map);

        assert_eq!(map.remove("foobar"), Some("barbaz"));
        assert_eq!(map.remove("foobar"), None);

        dump_superhashmap(&map);
    }
}
