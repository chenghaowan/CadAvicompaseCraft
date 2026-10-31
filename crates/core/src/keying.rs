use std::collections::HashSet;

#[derive(Clone)]
pub struct Shot { pub width: i32, pub height: i32, pub rgba: Vec<u8> }


/// 泛洪判定相邻像素同色系的容差(每通道)。只用于生长背景色板,不直接决定去留。
/// 调小一点(8):避免从背景"渗"进颜色接近的物体侧面(如 CAD 的灰色侧壁)。
const FLOOD_TOLERANCE: i32 = 8;
/// 背景键种子容差:只让背景聚类附近的颜色参与泛洪;太大会把物体边缘色也当成背景
const ANCHOR_SEED_TOLERANCE: i32 = 14;
/// 未与边框背景连通的区域:按"连通块平均色距"判定。
/// 真正的封闭背景块(表格格/孔底)平均色距≈0~2,整块去掉;
/// 孔壁高光/零件侧面虽有部分像素和背景同色,但平均色距更大,整块保留。
const ENCLOSED_AVG_TOLERANCE: f32 = 3.0;
/// 键控容差:与最近背景色的每通道差 ≤ 此值 → 完全按背景抹白
const KEY_TOLERANCE: i32 = 14;
/// 软键边宽度:容差外这一圈按比例向白混合,消除物体边缘的抗锯齿彩边
const KEY_FEATHER: i32 = 10;

/// 颜色量化:6 bit/通道(4 单位一格)
const QBITS: u32 = 6;
const QSHIFT: u32 = 8 - QBITS;
const QSIZE: i32 = 1 << QBITS;
const QBIN: i32 = 1 << QSHIFT;
const QN: usize = (QSIZE * QSIZE * QSIZE) as usize;
/// 距离场铺开的格数:足够覆盖「容差 + 软键边」再留一格余量
const MAX_BINS: i32 = 8;

fn qbin(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32 >> QSHIFT) << (2 * QBITS))
        | ((g as u32 >> QSHIFT) << QBITS)
        | (b as u32 >> QSHIFT)
}

/// 色格中心色:用于精确到像素的颜色距离(只有"最近色格"被量化,距离本身不量化)
fn bin_center(k: u32) -> (i32, i32, i32) {
    let mask = (QSIZE - 1) as u32;
    let half = QBIN / 2;
    (
        (((k >> (2 * QBITS)) & mask) as i32) * QBIN + half,
        (((k >> QBITS) & mask) as i32) * QBIN + half,
        ((k & mask) as i32) * QBIN + half,
    )
}

fn cheb(a: (i32, i32, i32), b: (i32, i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs()).max((a.2 - b.2).abs())
}

const DARKEN_LUMINANCE: f32 = 166.0;
/// 只有白底占主导时才反相;键控没去掉多少背景时保持原色更安全
const DARKEN_MIN_BG: f32 = 0.50;

/// 键控结果 + 决策依据
pub struct KeyingOutcome {
    pub rgba: Vec<u8>,
    /// 被键控完全判为背景的像素占全图比例
    pub bg_fraction: f32,
}

/// 键控去背 + 统计背景占比(Auto 模式用来决定是否交给 ISNet)。
/// `darken_light`:白底占主导时,把亮度高的前景(深底上的浅灰文字)反相压暗。
pub fn keying_whiten(shot: &Shot, darken_light: bool) -> KeyingOutcome {
    let w = shot.width.max(1) as usize;
    let h = shot.height.max(1) as usize;
    if shot.rgba.len() < w * h * 4 {
        return KeyingOutcome {
            rgba: shot.rgba.clone(),
            bg_fraction: 0.0,
        };
    }
    let (palette, visited) = background_palette(shot, w, h);
    if palette.is_empty() {
        return KeyingOutcome {
            rgba: shot.rgba.clone(),
            bg_fraction: 0.0,
        };
    }
    let nearest = nearest_key_grid(&palette);
    let (rgba, bg_pixels) = key_out(shot, w, h, &nearest, &visited, darken_light);
    KeyingOutcome {
        rgba,
        bg_fraction: bg_pixels as f32 / (w * h) as f32,
    }
}

/// 亮度(0-255,Rec.601 权重)
fn luminance(p: &[u8]) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// 浅色前景反相:保留色相、把亮度翻到暗部。
/// 例:200,200,200(浅灰文字)→ 55,55,55;255,255,0(浅黄)→ 深橄榄色。
fn darken_light_pixel(p: &mut [u8]) {
    let l = luminance(p);
    if l < DARKEN_LUMINANCE {
        return;
    }
    let target = 255.0 - l;
    let scale = if l > 1.0 { target / l } else { 0.0 };
    for c in 0..3 {
        p[c] = (p[c] as f32 * scale).round().clamp(0.0, 255.0) as u8;
    }
}

/// 只压暗"小而孤立"的浅色前景组件(文字/细线)。
/// 大块浅色物体(浅色零件面、白色面板)保持原色;贴边的浅色组件(白边/贴边文字)
/// 也不动,避免把截图边框反相成黑框。
fn darken_small_light_components(out: &mut [u8], mask: &[bool], w: usize, h: usize) {
    // 组件面积上限:全图 2%(至少 512 像素)。超过就认为是大块物体,不反相。
    let max_area = (w * h / 50).max(512);
    let mut seen = vec![false; w * h];
    let mut stack: Vec<u32> = Vec::new();
    let mut pixels: Vec<u32> = Vec::new();
    for start in 0..w * h {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.clear();
        stack.push(start as u32);
        pixels.clear();
        let mut touches_border = false;
        let mut too_big = false;
        while let Some(iu) = stack.pop() {
            let i = iu as usize;
            let x = i % w;
            let y = i / w;
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                touches_border = true;
            }
            if !too_big {
                pixels.push(iu);
                if pixels.len() > max_area {
                    too_big = true;
                    pixels.clear();
                }
            }
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let ni = ny as usize * w + nx as usize;
                    if mask[ni] && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni as u32);
                    }
                }
            }
        }
        if !too_big && !touches_border {
            for &iu in &pixels {
                let b = iu as usize * 4;
                darken_light_pixel(&mut out[b..b + 3]);
            }
        }
    }
}

/// 从边框取背景键并泛洪出完整背景色板;`visited` 是从边框连通出去的背景区域,
/// 用来区分"背景本体"和"颜色接近背景、但被物体包住的侧面/孔壁"。
fn background_palette(shot: &Shot, w: usize, h: usize) -> (HashSet<u32>, Vec<bool>) {
    let src = &shot.rgba;

    // 选框四边像素(去重)。跳过最外 2px:截图/导出图经常带一圈白边或选框描边,
    // 直接拿最外圈会把"白边"当成背景键,真正的背景反而完全没去掉。
    let inset = if w > 8 && h > 8 { 2 } else { 0 };
    let (x0, x1) = (inset, w - 1 - inset);
    let (y0, y1) = (inset, h - 1 - inset);
    let mut border: Vec<usize> = Vec::with_capacity(2 * (w + h));
    for x in x0..=x1 {
        border.push(y0 * w + x);
        if y1 != y0 {
            border.push(y1 * w + x);
        }
    }
    for y in y0 + 1..y1 {
        border.push(y * w + x0);
        if x1 != x0 {
            border.push(y * w + x1);
        }
    }

    // 边框颜色聚类:6bit 直方图会被渐变背景拆成很多小格,反而让"占满边框的大块物体"
    // 胜出。这里用 4bit 粗直方图 + 3×3×3 邻域合并,再按邻域像素数取最多的颜色作为背景键。
    const ANCHOR_BITS: u32 = 4;
    const ANCHOR_SIZE: i32 = 1 << ANCHOR_BITS;
    const ANCHOR_SHIFT: u32 = 8 - ANCHOR_BITS;
    const ANCHOR_N: usize = (ANCHOR_SIZE * ANCHOR_SIZE * ANCHOR_SIZE) as usize;
    let abin = |r: u8, g: u8, b: u8| -> usize {
        (((r as usize) >> ANCHOR_SHIFT) << (2 * ANCHOR_BITS))
            | (((g as usize) >> ANCHOR_SHIFT) << ANCHOR_BITS)
            | ((b as usize) >> ANCHOR_SHIFT)
    };
    let mut coarse = vec![0u32; ANCHOR_N];
    for &i in &border {
        let b = i * 4;
        coarse[abin(src[b], src[b + 1], src[b + 2])] += 1;
    }

    let mut anchor_rgb = (0, 0, 0);
    let mut best = 0u64;
    for r in 0..ANCHOR_SIZE {
        for g in 0..ANCHOR_SIZE {
            for b in 0..ANCHOR_SIZE {
                let (mut sum, mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64, 0u64);
                for dr in -1i32..=1 {
                    let rr = r + dr;
                    if !(0..ANCHOR_SIZE).contains(&rr) {
                        continue;
                    }
                    for dg in -1i32..=1 {
                        let gg = g + dg;
                        if !(0..ANCHOR_SIZE).contains(&gg) {
                            continue;
                        }
                        for db in -1i32..=1 {
                            let bb = b + db;
                            if !(0..ANCHOR_SIZE).contains(&bb) {
                                continue;
                            }
                            let idx = ((rr as usize) << (2 * ANCHOR_BITS))
                                | ((gg as usize) << ANCHOR_BITS)
                                | (bb as usize);
                            let c = coarse[idx] as u64;
                            if c == 0 {
                                continue;
                            }
                            sum += c;
                            sr += c * ((rr as u64) * 16 + 8);
                            sg += c * ((gg as u64) * 16 + 8);
                            sb += c * ((bb as u64) * 16 + 8);
                        }
                    }
                }
                if sum > best {
                    best = sum;
                    anchor_rgb = (
                        (sr / sum) as i32,
                        (sg / sum) as i32,
                        (sb / sum) as i32,
                    );
                }
            }
        }
    }
    if std::env::var("POINTTOFIX_DEBACK_LOG").is_ok() {
        let mut top: Vec<(usize, u32)> = coarse
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(i, c)| (i, *c))
            .collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        let top: Vec<_> = top
            .iter()
            .take(5)
            .map(|(i, c)| {
                let r = (i >> (2 * ANCHOR_BITS)) & (ANCHOR_SIZE as usize - 1);
                let g = (i >> ANCHOR_BITS) & (ANCHOR_SIZE as usize - 1);
                let b = i & (ANCHOR_SIZE as usize - 1);
                ((r * 16 + 8, g * 16 + 8, b * 16 + 8), *c)
            })
            .collect();
        eprintln!(
            "[keying] border={} anchor={:?} cluster_pixels={} top={:?}",
            border.len(),
            anchor_rgb,
            best,
            top
        );
    }

    // 种子:边框上颜色接近背景键的像素
    let mut visited = vec![false; w * h];
    let mut queue: Vec<usize> = Vec::with_capacity(w * h / 4);
    for &i in &border {
        let b = i * 4;
        let px = (src[b] as i32, src[b + 1] as i32, src[b + 2] as i32);
        if cheb(px, anchor_rgb) <= ANCHOR_SEED_TOLERANCE {
            visited[i] = true;
            queue.push(i);
        }
    }

    // 相邻色容差泛洪:让渐变背景一路连到底
    let mut head = 0usize;
    while head < queue.len() {
        let i = queue[head];
        head += 1;
        let x = i % w;
        let y = i / w;
        let b0 = i * 4;
        let cur = (
            src[b0] as i32,
            src[b0 + 1] as i32,
            src[b0 + 2] as i32,
        );

        let mut neighbors = [0usize; 4];
        let mut n = 0usize;
        if x > 0 {
            neighbors[n] = i - 1;
            n += 1;
        }
        if x + 1 < w {
            neighbors[n] = i + 1;
            n += 1;
        }
        if y > 0 {
            neighbors[n] = i - w;
            n += 1;
        }
        if y + 1 < h {
            neighbors[n] = i + w;
            n += 1;
        }

        for &ni in &neighbors[..n] {
            if visited[ni] {
                continue;
            }
            let b2 = ni * 4;
            let npx = (
                src[b2] as i32,
                src[b2 + 1] as i32,
                src[b2 + 2] as i32,
            );
            if cheb(npx, cur) <= FLOOD_TOLERANCE {
                visited[ni] = true;
                queue.push(ni);
            }
        }
    }

    // 只取"内部"背景像素上色:物体边缘的抗锯齿过渡色也算被泛洪到过,
    // 但它们离物体太近,收进色板会把物体边缘一起键掉。
    let mut palette: HashSet<u32> = HashSet::new();
    let mut interior = 0usize;
    for i in 0..w * h {
        if !visited[i] {
            continue;
        }
        let x = i % w;
        let y = i / w;
        let inside = (x == 0 || visited[i - 1])
            && (x + 1 == w || visited[i + 1])
            && (y == 0 || visited[i - w])
            && (y + 1 == h || visited[i + w]);
        if inside {
            interior += 1;
            let b = i * 4;
            palette.insert(qbin(src[b], src[b + 1], src[b + 2]));
        }
    }
    // 背景只有很细的一圈(内部像素太少)时退回全部背景像素
    if interior * 4 < w * h {
        for (i, &is_bg) in visited.iter().enumerate() {
            if is_bg {
                let b = i * 4;
                palette.insert(qbin(src[b], src[b + 1], src[b + 2]));
            }
        }
    }
    if std::env::var("POINTTOFIX_DEBACK_LOG").is_ok() {
        let visited_n = visited.iter().filter(|v| **v).count();
        eprintln!(
            "[keying] seeds={} visited={} interior={} palette={}",
            queue.len(),
            visited_n,
            interior,
            palette.len()
        );
    }
    (palette, visited)
}

/// 颜色空间 Chebyshev 距离场:每个色格记录最近背景色格的编号
fn nearest_key_grid(palette: &HashSet<u32>) -> Vec<u32> {
    let mut nearest = vec![u32::MAX; QN];
    let mut queue: Vec<u32> = Vec::with_capacity(palette.len() * 8);
    for &k in palette {
        if nearest[k as usize] == u32::MAX {
            nearest[k as usize] = k;
            queue.push(k);
        }
    }

    let mask = (QSIZE - 1) as u32;
    let mut head = 0usize;
    let mut layer_end = queue.len();
    let mut depth = 0i32;
    while head < queue.len() && depth < MAX_BINS {
        while head < layer_end {
            let k = queue[head];
            head += 1;
            let key_bin = nearest[k as usize];
            let r = (k >> (2 * QBITS)) & mask;
            let g = (k >> QBITS) & mask;
            let b = k & mask;
            for dr in -1i32..=1 {
                let rr = r as i32 + dr;
                if !(0..QSIZE).contains(&rr) {
                    continue;
                }
                for dg in -1i32..=1 {
                    let gg = g as i32 + dg;
                    if !(0..QSIZE).contains(&gg) {
                        continue;
                    }
                    for db in -1i32..=1 {
                        let bb = b as i32 + db;
                        if !(0..QSIZE).contains(&bb) {
                            continue;
                        }
                        let n = ((rr as u32) << (2 * QBITS))
                            | ((gg as u32) << QBITS)
                            | (bb as u32);
                        if nearest[n as usize] == u32::MAX {
                            nearest[n as usize] = key_bin;
                            queue.push(n);
                        }
                    }
                }
            }
        }
        layer_end = queue.len();
        depth += 1;
    }
    nearest
}

/// 按键控判定整幅图:全白 / 软键边混合 / 原样保留;返回 (结果, 完全判为背景的像素数)
fn key_out(
    shot: &Shot,
    w: usize,
    h: usize,
    nearest: &[u32],
    visited: &[bool],
    darken_light: bool,
) -> (Vec<u8>, usize) {
    let mut out = shot.rgba.clone();
    let full = KEY_TOLERANCE + KEY_FEATHER;
    let mut bg_pixels = 0usize;
    // 完全保留的浅色前景(通常是深色背景上的浅灰文字),等确认白底占主导后再压暗
    let mut light_mask = vec![false; w * h];
    // 未与边框背景连通、但颜色落在背景容差内的像素:稍后按连通块整体判定
    let mut candidate = vec![false; w * h];
    let mut dist = vec![0u8; w * h];

    for i in 0..w * h {
        let b = i * 4;
        let px = (out[b] as i32, out[b + 1] as i32, out[b + 2] as i32);
        let key = nearest[qbin(out[b], out[b + 1], out[b + 2]) as usize];
        if key == u32::MAX {
            // 离背景色板太远(超出距离场):一定是前景。深底上的浅色文字通常落在这里
            if darken_light && luminance(&out[b..b + 3]) >= DARKEN_LUMINANCE {
                light_mask[i] = true;
            }
            continue;
        }
        let d = cheb(px, bin_center(key));
        if d > full {
            if darken_light && luminance(&out[b..b + 3]) >= DARKEN_LUMINANCE {
                light_mask[i] = true;
            }
            continue;
        }
        if !visited[i] {
            // 先记录下来,等整个连通块的平均色距算完再决定
            candidate[i] = true;
            dist[i] = d.min(255) as u8;
            continue;
        }
        if d <= KEY_TOLERANCE {
            out[b] = 255;
            out[b + 1] = 255;
            out[b + 2] = 255;
            out[b + 3] = 255;
            bg_pixels += 1;
        } else {
            // 软键边:越靠近容差越接近纯白,到 full 处完全保留原色
            let n = (full - d) as u32;
            let f = KEY_FEATHER as u32;
            for c in 0..3 {
                let v = out[b + c] as u32;
                out[b + c] = ((v * (f - n) + 255 * n) / f) as u8;
            }
            out[b + 3] = 255;
        }
    }

    // 按连通块判定未连通区域:平均色距很小的整块是封闭背景(表格格/孔底),
    // 孔壁高光、零件侧面这类"部分像素接近背景"的块整体保留。
    {
        let mut seen = vec![false; w * h];
        let mut stack: Vec<u32> = Vec::new();
        let mut pixels: Vec<u32> = Vec::new();
        for start in 0..w * h {
            if !candidate[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            stack.clear();
            stack.push(start as u32);
            pixels.clear();
            let mut sum = 0u32;
            while let Some(iu) = stack.pop() {
                let i = iu as usize;
                pixels.push(iu);
                sum += dist[i] as u32;
                let x = i % w;
                let y = i / w;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let ni = ny as usize * w + nx as usize;
                        if candidate[ni] && !seen[ni] {
                            seen[ni] = true;
                            stack.push(ni as u32);
                        }
                    }
                }
            }
            let avg = sum as f32 / pixels.len().max(1) as f32;
            if avg <= ENCLOSED_AVG_TOLERANCE {
                for &iu in &pixels {
                    let b = iu as usize * 4;
                    out[b] = 255;
                    out[b + 1] = 255;
                    out[b + 2] = 255;
                    out[b + 3] = 255;
                    bg_pixels += 1;
                }
            } else if darken_light {
                // 保留的块里如果有浅色像素,照常参与"小文字转黑"
                for &iu in &pixels {
                    let b = iu as usize * 4;
                    if luminance(&out[b..b + 3]) >= DARKEN_LUMINANCE {
                        light_mask[iu as usize] = true;
                    }
                }
            }
        }
    }

    let bg_fraction = bg_pixels as f32 / (w * h).max(1) as f32;
    if darken_light && bg_fraction >= DARKEN_MIN_BG {
        darken_small_light_components(&mut out, &light_mask, w, h);
    }
    (out, bg_pixels)
}
