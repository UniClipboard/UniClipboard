/**
 * 通用 web stub —— 用于 metro 在 web 平台把 native-only 模块映射到这里
 * 通过 Proxy 兼容任意 named import / default import / type import 用法
 */

const noop = () => undefined;

const handler = {
  get(target, prop) {
    if (prop === '__esModule') return true;
    if (typeof prop === 'symbol') return undefined;
    if (prop in target) return target[prop];
    // 任意属性访问返回一个既可调用、又可继续访问属性的 Proxy
    const child = new Proxy(noop, handler);
    target[prop] = child;
    return child;
  },
  apply() {
    return undefined;
  },
};

const stub = new Proxy(function () {}, handler);

module.exports = stub;
module.exports.default = stub;
