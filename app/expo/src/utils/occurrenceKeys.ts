/**
 * 网格渲染 key 消歧:业务 key(profileHash)理论上唯一,但脏数据
 * (历史上导入/同步写入的重复记录)出现过同 hash 多条的情况。
 * React 列表 key 冲突会让组件实例被"过继"给另一份副本,虚拟化滚动时
 * 表现为卡片乱飞、位置空洞。此函数按出现序为重复 key 加后缀,
 * 保证渲染 key 全局唯一且对相同输入稳定;首份副本保留原始 key,
 * 正常(无重复)数据的动画身份完全不受影响。
 */
export function buildOccurrenceKeys<T>(items: T[], keyExtractor: (item: T) => string): string[] {
  const seen = new Map<string, number>();
  return items.map((item) => {
    const base = keyExtractor(item);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    return n === 0 ? base : `${base}~dup${n}`;
  });
}
