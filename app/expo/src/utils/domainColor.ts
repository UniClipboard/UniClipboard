/**
 * 域名派生色：把域名哈希到一对稳定的深色渐变色，
 * 作为链接卡片无 OG 图时的兜底背景。深色调保证白色覆层文字可读。
 */

export interface DomainGradient {
  start: string;
  end: string;
}

export function getDomainGradient(domain: string): DomainGradient {
  let hash = 0;
  for (let i = 0; i < domain.length; i++) {
    hash = (hash * 31 + domain.charCodeAt(i)) >>> 0;
  }
  const hue = hash % 360;
  return {
    start: hslToHex(hue, 52, 46),
    end: hslToHex((hue + 42) % 360, 56, 36),
  };
}

/** favicon 缺失/加载失败时的字标：域名（去 www.）首字符大写 */
export function getDomainInitial(domain: string): string {
  const clean = domain.replace(/^www\./i, '');
  const ch = clean.charAt(0);
  return ch ? ch.toUpperCase() : '#';
}

// react-native-svg 的 Stop 对 hsl 字符串支持不稳，统一输出 hex
function hslToHex(h: number, s: number, l: number): string {
  const sn = s / 100;
  const ln = l / 100;
  const a = sn * Math.min(ln, 1 - ln);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const color = ln - a * Math.max(-1, Math.min(k - 3, Math.min(9 - k, 1)));
    return Math.round(255 * color)
      .toString(16)
      .padStart(2, '0');
  };
  return `#${f(0)}${f(8)}${f(4)}`;
}
