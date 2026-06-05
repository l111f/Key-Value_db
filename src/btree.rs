use std::cmp::Ordering;

use crate::buffer_pool::BufferPoolManager;
use crate::error::Result;
use crate::page::PAGE_SIZE;

/// 节点类型枚举
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Internal,
    Leaf,
}

/// B+ 树节点
#[derive(Debug, Clone)]
pub struct BPlusTreeNode {
    /// 节点类型
    pub node_type: NodeType,
    /// 所在页号
    pub page_id: u32,
    /// 键列表，按字节序升序排列
    pub keys: Vec<Vec<u8>>,
    /// 子节点页号列表（仅内部节点）
    pub children: Vec<u32>,
    /// 值列表（仅叶子节点）
    pub values: Vec<Vec<u8>>,
    /// 右兄弟叶子节点页号（仅叶子节点）
    pub next_leaf: Option<u32>,
    /// 父节点页号
    pub parent: Option<u32>,
}

const PARENT_NONE: u32 = 0xFFFFFFFF;
const NEXT_LEAF_NONE: u32 = 0xFFFFFFFF;

impl BPlusTreeNode {
    /// 创建新的叶子节点
    pub fn new_leaf(page_id: u32) -> Self {
        BPlusTreeNode {
            node_type: NodeType::Leaf,
            page_id,
            keys: Vec::new(),
            children: Vec::new(),
            values: Vec::new(),
            next_leaf: None,
            parent: None,
        }
    }

    /// 创建新的内部节点
    pub fn new_internal(page_id: u32) -> Self {
        BPlusTreeNode {
            node_type: NodeType::Internal,
            page_id,
            keys: Vec::new(),
            children: Vec::new(),
            values: Vec::new(),
            next_leaf: None,
            parent: None,
        }
    }

    /// 序列化节点到字节数组
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(PAGE_SIZE);

        // 节点类型
        buf.push(match self.node_type {
            NodeType::Internal => 0x01,
            NodeType::Leaf => 0x02,
        });

        // 父节点页号
        let parent = self.parent.unwrap_or(PARENT_NONE);
        buf.extend_from_slice(&parent.to_le_bytes());

        // 键数量
        let key_count = self.keys.len() as u16;
        buf.extend_from_slice(&key_count.to_le_bytes());

        // next_leaf
        let next_leaf = self.next_leaf.unwrap_or(NEXT_LEAF_NONE);
        buf.extend_from_slice(&next_leaf.to_le_bytes());

        match self.node_type {
            NodeType::Internal => {
                // children[0]
                if !self.children.is_empty() {
                    buf.extend_from_slice(&self.children[0].to_le_bytes());
                }
                for (i, key) in self.keys.iter().enumerate() {
                    buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
                    buf.extend_from_slice(key);
                    if i + 1 < self.children.len() {
                        buf.extend_from_slice(&self.children[i + 1].to_le_bytes());
                    }
                }
            }
            NodeType::Leaf => {
                for (i, key) in self.keys.iter().enumerate() {
                    buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
                    buf.extend_from_slice(key);
                    if i < self.values.len() {
                        let value = &self.values[i];
                        buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
                        buf.extend_from_slice(value);
                    }
                }
            }
        }

        buf
    }

    /// 从 Page 数据区反序列化节点
    pub fn deserialize(page_id: u32, data: &[u8]) -> Result<Self> {
        let node_data_start = 11;
        if data.len() < node_data_start {
            return Ok(BPlusTreeNode::new_leaf(page_id));
        }

        let mut offset = 0;

        let node_type_byte = data[offset];
        offset += 1;
        let node_type = match node_type_byte {
            0x01 => NodeType::Internal,
            0x02 => NodeType::Leaf,
            _ => NodeType::Leaf,
        };

        let parent = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
        offset += 4;
        let parent = if parent == PARENT_NONE { None } else { Some(parent) };

        let key_count = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;

        let next_leaf = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
        offset += 4;
        let next_leaf = if next_leaf == NEXT_LEAF_NONE { None } else { Some(next_leaf) };

        let mut keys: Vec<Vec<u8>> = Vec::with_capacity(key_count);
        let mut children: Vec<u32> = Vec::new();
        let mut values: Vec<Vec<u8>> = Vec::new();

        match node_type {
            NodeType::Internal => {
                if offset + 4 <= data.len() && key_count > 0 {
                    let child0 = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                    offset += 4;
                    children.push(child0);
                }
                for _ in 0..key_count {
                    if offset + 4 > data.len() { break; }
                    let key_len = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
                    offset += 4;
                    if offset + key_len > data.len() { break; }
                    keys.push(data[offset..offset + key_len].to_vec());
                    offset += key_len;
                    if offset + 4 > data.len() { break; }
                    let child = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]);
                    offset += 4;
                    children.push(child);
                }
            }
            NodeType::Leaf => {
                for _ in 0..key_count {
                    if offset + 4 > data.len() { break; }
                    let key_len = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
                    offset += 4;
                    if offset + key_len > data.len() { break; }
                    keys.push(data[offset..offset + key_len].to_vec());
                    offset += key_len;
                    if offset + 4 > data.len() { break; }
                    let value_len = u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]) as usize;
                    offset += 4;
                    if offset + value_len > data.len() { break; }
                    values.push(data[offset..offset + value_len].to_vec());
                    offset += value_len;
                }
            }
        }

        Ok(BPlusTreeNode {
            node_type,
            page_id,
            keys,
            children,
            values,
            next_leaf,
            parent,
        })
    }
}

/// 键比较函数：字节序逐字节比较
pub fn compare_keys(a: &[u8], b: &[u8]) -> Ordering {
    let min_len = a.len().min(b.len());
    for i in 0..min_len {
        match a[i].cmp(&b[i]) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    a.len().cmp(&b.len())
}

/// 查找 key 应该在的位置（第一个 > key 的位置）
fn find_position(keys: &[Vec<u8>], key: &[u8]) -> usize {
    keys.iter().position(|k| compare_keys(key, k) == Ordering::Less).unwrap_or(keys.len())
}

/// B+ 树结构
pub struct BPlusTree {
    /// 根节点页号
    root_page_id: Option<u32>,
    /// 阶数
    order: usize,
    /// 缓冲池管理器
    buffer_pool: BufferPoolManager,
}

impl BPlusTree {
    /// 创建空 B+树
    pub fn new(order: usize, buffer_pool: BufferPoolManager) -> Self {
        // 尝试从磁盘读取已保存的 root_page_id
        let root_page_id = Self::load_root_from_disk(&buffer_pool);

        BPlusTree {
            root_page_id,
            order,
            buffer_pool,
        }
    }

    /// 从磁盘文件读取 root_page_id
    fn load_root_from_disk(buffer_pool: &BufferPoolManager) -> Option<u32> {
        let db_path = buffer_pool.get_disk_manager_ref().get_db_path();
        let meta_path = db_path.replace(".db", ".meta");
        if std::path::Path::new(&meta_path).exists() {
            if let Ok(data) = std::fs::read(&meta_path) {
                if data.len() >= 4 {
                    let root = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    if root != 0xFFFFFFFF {
                        return Some(root);
                    }
                }
            }
        }
        None
    }

    /// 保存 root_page_id 到磁盘文件
    fn save_metadata(&mut self) -> Result<()> {
        let db_path = self.buffer_pool.get_disk_manager_ref().get_db_path();
        let meta_path = db_path.replace(".db", ".meta");
        let root = self.root_page_id.unwrap_or(0xFFFFFFFF);
        std::fs::write(&meta_path, &root.to_le_bytes())?;
        Ok(())
    }

    fn min_keys(&self) -> usize {
        (self.order + 1) / 2 - 1
    }

    fn max_keys(&self) -> usize {
        self.order - 1
    }

    /// 将节点写入缓冲区
    fn write_node(&mut self, node: &BPlusTreeNode) -> Result<()> {
        let serialized = node.serialize();
        let mut page_data = [0u8; PAGE_SIZE];

        match node.node_type {
            NodeType::Internal => page_data[0] = 0x01,
            NodeType::Leaf => page_data[0] = 0x02,
        }

        let key_count = node.keys.len() as u16;
        page_data[5..7].copy_from_slice(&key_count.to_le_bytes());

        let data_start = 8;
        let ser_len = serialized.len().min(PAGE_SIZE - data_start);
        page_data[data_start..data_start + ser_len].copy_from_slice(&serialized[..ser_len]);

        self.buffer_pool.write_page_data(node.page_id, &page_data)?;
        self.buffer_pool.mark_dirty(node.page_id);
        Ok(())
    }

    /// 从缓冲区读取节点
    fn read_node(&mut self, page_id: u32) -> Result<BPlusTreeNode> {
        let page = self.buffer_pool.fetch_page(page_id)?;
        let node = BPlusTreeNode::deserialize(page_id, &page.data[8..])?;
        Ok(node)
    }

    /// 释放页（取消 pin）
    fn release_node(&mut self, page_id: u32) {
        self.buffer_pool.unpin(page_id);
    }

    /// 精确查找
    pub fn search(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        let root_id = self.root_page_id?;
        self.search_in_node(root_id, key)
    }

    fn search_in_node(&mut self, page_id: u32, key: &[u8]) -> Option<Vec<u8>> {
        let node = self.read_node(page_id).ok()?;
        match node.node_type {
            NodeType::Internal => {
                let child_idx = find_position(&node.keys, key);
                let child_page_id = node.children.get(child_idx).copied();
                self.release_node(page_id);
                child_page_id.and_then(|id| self.search_in_node(id, key))
            }
            NodeType::Leaf => {
                let found = node.keys.iter().position(|k| compare_keys(key, k) == Ordering::Equal);
                let result = found.and_then(|idx| node.values.get(idx).cloned());
                self.release_node(page_id);
                result
            }
        }
    }

    /// 插入键值对
    pub fn insert(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        match self.root_page_id {
            None => {
                let (page_id, _) = self.buffer_pool.create_page()?;
                let mut root = BPlusTreeNode::new_leaf(page_id);
                root.keys.push(key.to_vec());
                root.values.push(value.to_vec());
                self.write_node(&root)?;
                self.release_node(page_id);
                self.root_page_id = Some(page_id);
                self.save_metadata()?;
                Ok(())
            }
            Some(root_id) => {
                let split_result = self.insert_recursive(root_id, key, value)?;
                if let Some((mid_key, right_page_id)) = split_result {
                    let (new_root_id, _) = self.buffer_pool.create_page()?;
                    let mut new_root = BPlusTreeNode::new_internal(new_root_id);
                    new_root.keys.push(mid_key);
                    new_root.children.push(root_id);
                    new_root.children.push(right_page_id);

                    self.update_parent(root_id, Some(new_root_id))?;
                    self.update_parent(right_page_id, Some(new_root_id))?;

                    self.write_node(&new_root)?;
                    self.release_node(new_root_id);
                    self.root_page_id = Some(new_root_id);
                    self.save_metadata()?;
                }
                Ok(())
            }
        }
    }

    fn insert_recursive(
        &mut self,
        page_id: u32,
        key: &[u8],
        value: &[u8],
    ) -> Result<Option<(Vec<u8>, u32)>> {
        let mut node = self.read_node(page_id)?;

        match node.node_type {
            NodeType::Leaf => {
                // 查找 key 是否已存在（精确匹配）
                let exact_pos = node.keys.iter().position(|k| compare_keys(key, k) == Ordering::Equal);
                if let Some(pos) = exact_pos {
                    // 更新已有键
                    node.values[pos] = value.to_vec();
                    self.write_node(&node)?;
                    self.release_node(page_id);
                    return Ok(None);
                }

                let pos = find_position(&node.keys, key);

                node.keys.insert(pos, key.to_vec());
                node.values.insert(pos, value.to_vec());

                if node.keys.len() > self.max_keys() {
                    // 叶子节点分裂
                    let mid = node.keys.len() / 2;
                    let split_key = node.keys[mid].clone();

                    let (new_page_id, _) = self.buffer_pool.create_page()?;
                    let mut right_node = BPlusTreeNode::new_leaf(new_page_id);
                    right_node.keys = node.keys.split_off(mid);
                    right_node.values = node.values.split_off(mid);
                    right_node.next_leaf = node.next_leaf;
                    right_node.parent = node.parent;
                    node.next_leaf = Some(new_page_id);

                    self.write_node(&node)?;
                    self.write_node(&right_node)?;
                    self.release_node(page_id);
                    self.release_node(new_page_id);

                    Ok(Some((split_key, new_page_id)))
                } else {
                    self.write_node(&node)?;
                    self.release_node(page_id);
                    Ok(None)
                }
            }
            NodeType::Internal => {
                let child_idx = find_position(&node.keys, key);
                let child_page_id = node.children[child_idx];
                self.release_node(page_id);

                let split_result = self.insert_recursive(child_page_id, key, value)?;

                if let Some((mid_key, new_child_id)) = split_result {
                    let mut node = self.read_node(page_id)?;
                    let pos = find_position(&node.keys, &mid_key);
                    node.keys.insert(pos, mid_key);
                    node.children.insert(pos + 1, new_child_id);

                    self.update_parent(new_child_id, Some(page_id))?;

                    if node.keys.len() > self.max_keys() {
                        // 内部节点分裂
                        let mid = node.keys.len() / 2;
                        let promote_key = node.keys[mid].clone();

                        let (new_page_id, _) = self.buffer_pool.create_page()?;
                        let mut right_node = BPlusTreeNode::new_internal(new_page_id);

                        // 分裂：右节点取 mid+1.. 的 keys 和 mid+1.. 的 children
                        let right_keys: Vec<Vec<u8>> = node.keys.drain(mid + 1..).collect();
                        let right_children: Vec<u32> = node.children.drain(mid + 1..).collect();
                        right_node.keys = right_keys;
                        right_node.children = right_children;
                        right_node.parent = node.parent;

                        // 移除被提升的键
                        node.keys.pop(); // mid 位置的键

                        for &cid in &right_node.children {
                            self.update_parent(cid, Some(new_page_id))?;
                        }

                        self.write_node(&node)?;
                        self.write_node(&right_node)?;
                        self.release_node(page_id);
                        self.release_node(new_page_id);

                        Ok(Some((promote_key, new_page_id)))
                    } else {
                        self.write_node(&node)?;
                        self.release_node(page_id);
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// 更新节点的父指针
    fn update_parent(&mut self, page_id: u32, parent: Option<u32>) -> Result<()> {
        let mut node = self.read_node(page_id)?;
        node.parent = parent;
        self.write_node(&node)?;
        self.release_node(page_id);
        Ok(())
    }

    /// 删除键
    pub fn remove(&mut self, key: &[u8]) -> Result<bool> {
        let root_id = match self.root_page_id {
            Some(id) => id,
            None => return Ok(false),
        };

        let removed = self.remove_recursive(root_id, key)?;

        // 检查根是否需要缩减
        if let Some(root_id) = self.root_page_id {
            let root = self.read_node(root_id)?;
            if root.node_type == NodeType::Internal && root.keys.is_empty() {
                let new_root_id = root.children[0];
                self.release_node(root_id);
                self.root_page_id = Some(new_root_id);
                self.update_parent(new_root_id, None)?;
                self.save_metadata()?;
            } else if root.node_type == NodeType::Leaf && root.keys.is_empty() {
                self.release_node(root_id);
                self.root_page_id = None;
                self.save_metadata()?;
            } else {
                self.release_node(root_id);
            }
        }

        Ok(removed)
    }

    fn remove_recursive(&mut self, page_id: u32, key: &[u8]) -> Result<bool> {
        let mut node = self.read_node(page_id)?;

        match node.node_type {
            NodeType::Leaf => {
                let pos = match node.keys.iter().position(|k| compare_keys(key, k) == Ordering::Equal) {
                    Some(p) => p,
                    None => {
                        self.release_node(page_id);
                        return Ok(false);
                    }
                };
                node.keys.remove(pos);
                node.values.remove(pos);
                self.write_node(&node)?;
                self.release_node(page_id);
                Ok(true)
            }
            NodeType::Internal => {
                let child_idx = find_position(&node.keys, key);
                let child_page_id = node.children[child_idx];
                self.release_node(page_id);

                let removed = self.remove_recursive(child_page_id, key)?;
                if !removed {
                    return Ok(false);
                }

                // 处理下溢
                self.handle_underflow(page_id)?;
                Ok(true)
            }
        }
    }

    fn handle_underflow(&mut self, parent_page_id: u32) -> Result<()> {
        let parent = self.read_node(parent_page_id)?;

        for i in 0..parent.children.len() {
            let child_id = parent.children[i];
            let child = self.read_node(child_id)?;

            if child.parent.is_none() {
                self.release_node(child_id);
                continue;
            }

            if child.keys.len() < self.min_keys() {
                self.release_node(child_id);
                self.release_node(parent_page_id);
                self.rebalance(parent_page_id, i)?;
                return Ok(());
            }
            self.release_node(child_id);
        }
        self.release_node(parent_page_id);
        Ok(())
    }

    fn rebalance(&mut self, parent_page_id: u32, child_idx: usize) -> Result<()> {
        let mut parent = self.read_node(parent_page_id)?;
        let child_id = parent.children[child_idx];
        let mut child = self.read_node(child_id)?;

        // 尝试从左兄弟借用
        if child_idx > 0 {
            let left_sibling_id = parent.children[child_idx - 1];
            let mut left_sibling = self.read_node(left_sibling_id)?;

            if left_sibling.keys.len() > self.min_keys() {
                match child.node_type {
                    NodeType::Leaf => {
                        let borrow_key = left_sibling.keys.last().unwrap().clone();
                        let borrow_value = left_sibling.values.last().unwrap().clone();
                        parent.keys[child_idx - 1] = borrow_key.clone();
                        child.keys.insert(0, borrow_key);
                        child.values.insert(0, borrow_value);
                    }
                    NodeType::Internal => {
                        let borrow_key = left_sibling.keys.pop().unwrap();
                        let borrow_child = left_sibling.children.pop().unwrap();
                        let parent_key = std::mem::replace(&mut parent.keys[child_idx - 1], borrow_key);
                        child.keys.insert(0, parent_key);
                        child.children.insert(0, borrow_child);
                        self.update_parent(child.children[0], Some(child_id))?;
                    }
                }

                if left_sibling.node_type == NodeType::Leaf {
                    left_sibling.keys.pop();
                    left_sibling.values.pop();
                }

                self.write_node(&parent)?;
                self.write_node(&child)?;
                self.write_node(&left_sibling)?;
                self.release_node(parent_page_id);
                self.release_node(child_id);
                self.release_node(left_sibling_id);
                return Ok(());
            }
            self.release_node(left_sibling_id);
        }

        // 尝试从右兄弟借用
        if child_idx + 1 < parent.children.len() {
            let right_sibling_id = parent.children[child_idx + 1];
            let mut right_sibling = self.read_node(right_sibling_id)?;

            if right_sibling.keys.len() > self.min_keys() {
                match child.node_type {
                    NodeType::Leaf => {
                        let borrow_key = right_sibling.keys.remove(0);
                        let borrow_value = right_sibling.values.remove(0);
                        if !right_sibling.keys.is_empty() {
                            parent.keys[child_idx] = right_sibling.keys[0].clone();
                        }
                        child.keys.push(borrow_key);
                        child.values.push(borrow_value);
                    }
                    NodeType::Internal => {
                        let borrow_child = right_sibling.children.remove(0);
                        let parent_key = std::mem::replace(&mut parent.keys[child_idx], right_sibling.keys.remove(0));
                        child.keys.push(parent_key);
                        child.children.push(borrow_child);
                        self.update_parent(*child.children.last().unwrap(), Some(child_id))?;
                    }
                }

                self.write_node(&parent)?;
                self.write_node(&child)?;
                self.write_node(&right_sibling)?;
                self.release_node(parent_page_id);
                self.release_node(child_id);
                self.release_node(right_sibling_id);
                return Ok(());
            }
            self.release_node(right_sibling_id);
        }

        self.release_node(child_id);
        self.release_node(parent_page_id);

        // 合并
        if child_idx > 0 {
            self.merge_nodes(parent_page_id, child_idx, true)?;
        } else if child_idx + 1 < {
            let p = self.read_node(parent_page_id)?;
            let len = p.children.len();
            self.release_node(parent_page_id);
            len
        } {
            self.merge_nodes(parent_page_id, child_idx, false)?;
        }

        Ok(())
    }

    fn merge_nodes(&mut self, parent_page_id: u32, child_idx: usize, merge_with_left: bool) -> Result<()> {
        let mut parent = self.read_node(parent_page_id)?;

        if merge_with_left {
            let left_id = parent.children[child_idx - 1];
            let right_id = parent.children[child_idx];

            let mut left = self.read_node(left_id)?;
            let mut right = self.read_node(right_id)?;

            match left.node_type {
                NodeType::Leaf => {
                    left.keys.append(&mut right.keys);
                    left.values.append(&mut right.values);
                    left.next_leaf = right.next_leaf;
                }
                NodeType::Internal => {
                    let parent_key = parent.keys.remove(child_idx - 1);
                    left.keys.push(parent_key);
                    left.keys.append(&mut right.keys);
                    left.children.append(&mut right.children);
                    for &cid in &left.children {
                        self.update_parent(cid, Some(left_id))?;
                    }
                }
            }

            parent.children.remove(child_idx);

            self.write_node(&left)?;
            self.write_node(&parent)?;
            self.release_node(left_id);
            self.release_node(right_id);
            self.release_node(parent_page_id);
        } else {
            let left_id = parent.children[child_idx];
            let right_id = parent.children[child_idx + 1];

            let mut left = self.read_node(left_id)?;
            let mut right = self.read_node(right_id)?;

            match left.node_type {
                NodeType::Leaf => {
                    left.keys.append(&mut right.keys);
                    left.values.append(&mut right.values);
                    left.next_leaf = right.next_leaf;
                }
                NodeType::Internal => {
                    let parent_key = parent.keys.remove(child_idx);
                    left.keys.push(parent_key);
                    left.keys.append(&mut right.keys);
                    left.children.append(&mut right.children);
                    for &cid in &left.children {
                        self.update_parent(cid, Some(left_id))?;
                    }
                }
            }

            parent.children.remove(child_idx + 1);

            self.write_node(&left)?;
            self.write_node(&parent)?;
            self.release_node(left_id);
            self.release_node(right_id);
            self.release_node(parent_page_id);
        }

        // 如果父节点下溢，递归处理
        let parent_node = self.read_node(parent_page_id)?;
        let needs_rebalance = parent_node.parent.is_some() && parent_node.keys.len() < self.min_keys();
        let parent_parent = parent_node.parent;
        self.release_node(parent_page_id);

        if needs_rebalance {
            if let Some(grandparent_id) = parent_parent {
                let grandparent = self.read_node(grandparent_id)?;
                if let Some(idx) = grandparent.children.iter().position(|&id| id == parent_page_id) {
                    self.release_node(grandparent_id);
                    self.rebalance(grandparent_id, idx)?;
                } else {
                    self.release_node(grandparent_id);
                }
            }
        }

        Ok(())
    }

    /// 范围扫描
    pub fn range_scan(&mut self, start: &[u8], end: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
        let root_id = match self.root_page_id {
            Some(id) => id,
            None => return Vec::new(),
        };

        let mut results = Vec::new();

        if let Some(leaf_id) = self.find_leaf(root_id, start) {
            let mut current_id = Some(leaf_id);

            while let Some(pid) = current_id {
                let node = match self.read_node(pid) {
                    Ok(n) => n,
                    Err(_) => break,
                };

                if node.node_type != NodeType::Leaf {
                    self.release_node(pid);
                    break;
                }

                for (i, key) in node.keys.iter().enumerate() {
                    if compare_keys(key, end) == Ordering::Greater {
                        self.release_node(pid);
                        return results;
                    }
                    if compare_keys(key, start) != Ordering::Less {
                        results.push((key.clone(), node.values[i].clone()));
                    }
                }

                current_id = node.next_leaf;
                self.release_node(pid);
            }
        }

        results
    }

    fn find_leaf(&mut self, page_id: u32, key: &[u8]) -> Option<u32> {
        let node = self.read_node(page_id).ok()?;
        match node.node_type {
            NodeType::Internal => {
                let child_idx = find_position(&node.keys, key);
                let child_id = node.children.get(child_idx).copied();
                self.release_node(page_id);
                child_id.and_then(|id| self.find_leaf(id, key))
            }
            NodeType::Leaf => {
                self.release_node(page_id);
                Some(page_id)
            }
        }
    }

    pub fn get_buffer_pool_mut(&mut self) -> &mut BufferPoolManager {
        &mut self.buffer_pool
    }

    pub fn get_root_page_id(&self) -> Option<u32> {
        self.root_page_id
    }

    pub fn flush_all(&mut self) -> Result<()> {
        self.buffer_pool.flush_all()
    }

    pub fn shutdown(&mut self) -> Result<()> {
        self.buffer_pool.shutdown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disk_manager::DiskManager;
    use tempfile;

    fn create_test_tree(order: usize) -> (tempfile::TempDir, BPlusTree) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_str().unwrap().to_string();
        let dm = DiskManager::new(&db_path).unwrap();
        let bpm = BufferPoolManager::new(100, dm);
        let tree = BPlusTree::new(order, bpm);
        (dir, tree)
    }

    #[test]
    fn test_empty_tree_search() {
        let (_dir, mut tree) = create_test_tree(4);
        assert_eq!(tree.search(b"key"), None);
    }

    #[test]
    fn test_insert_and_search() {
        let (_dir, mut tree) = create_test_tree(4);
        tree.insert(b"key1", b"value1").unwrap();
        tree.insert(b"key2", b"value2").unwrap();
        tree.insert(b"key3", b"value3").unwrap();
        assert_eq!(tree.search(b"key1"), Some(b"value1".to_vec()));
        assert_eq!(tree.search(b"key2"), Some(b"value2".to_vec()));
        assert_eq!(tree.search(b"key3"), Some(b"value3".to_vec()));
        assert_eq!(tree.search(b"key4"), None);
    }

    #[test]
    fn test_insert_update() {
        let (_dir, mut tree) = create_test_tree(4);
        tree.insert(b"key1", b"value1").unwrap();
        tree.insert(b"key1", b"value2").unwrap();
        assert_eq!(tree.search(b"key1"), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_leaf_split() {
        let (_dir, mut tree) = create_test_tree(4);
        for i in 0..10 {
            let key = format!("key{:02}", i);
            let value = format!("value{:02}", i);
            tree.insert(key.as_bytes(), value.as_bytes()).unwrap();
        }
        for i in 0..10 {
            let key = format!("key{:02}", i);
            let value = format!("value{:02}", i);
            assert_eq!(tree.search(key.as_bytes()), Some(value.as_bytes().to_vec()));
        }
    }

    #[test]
    fn test_range_scan() {
        let (_dir, mut tree) = create_test_tree(4);
        for i in 0..20 {
            let key = format!("key{:02}", i);
            let value = format!("value{:02}", i);
            tree.insert(key.as_bytes(), value.as_bytes()).unwrap();
        }
        let results = tree.range_scan(b"key05", b"key10");
        assert_eq!(results.len(), 6);
        for i in 0..results.len() - 1 {
            assert!(compare_keys(&results[i].0, &results[i + 1].0) == Ordering::Less);
        }
    }

    #[test]
    fn test_remove() {
        let (_dir, mut tree) = create_test_tree(4);
        tree.insert(b"key1", b"value1").unwrap();
        tree.insert(b"key2", b"value2").unwrap();
        tree.insert(b"key3", b"value3").unwrap();
        assert_eq!(tree.remove(b"key2").unwrap(), true);
        assert_eq!(tree.search(b"key2"), None);
        assert_eq!(tree.remove(b"key99").unwrap(), false);
    }

    #[test]
    fn test_large_insert_and_search() {
        let (_dir, mut tree) = create_test_tree(10);
        for i in 0..100 {
            let key = format!("key{:04}", i);
            let value = format!("value{:04}", i);
            tree.insert(key.as_bytes(), value.as_bytes()).unwrap();
        }
        for i in 0..100 {
            let key = format!("key{:04}", i);
            let value = format!("value{:04}", i);
            assert_eq!(tree.search(key.as_bytes()), Some(value.as_bytes().to_vec()));
        }
        let results = tree.range_scan(b"key0020", b"key0050");
        assert_eq!(results.len(), 31);
    }
}