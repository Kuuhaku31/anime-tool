
// view_watch/src/utils/dom.ts
// 提供安全创建和查询页面元素的方法.

/** 创建元素并使用文本内容而不是 HTML 渲染数据. */
export function element<K extends keyof HTMLElementTagNameMap>(
  tag: K, text?: string, className?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
}

/** 创建可访问的按钮. */
export function button(text: string, action: () => void, className?: string): HTMLButtonElement {
  const node = element("button", text, className);
  node.type = "button";
  node.addEventListener("click", action);
  return node;
}

/** 仅允许 HTTP 或 HTTPS 外链. */
export function link(url: string | null, label: string): HTMLElement {
  if (!url) return element("span", "-");
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return element("span", "无效链接");
    const anchor = element("a", label);
    anchor.href = parsed.href;
    anchor.target = "_blank";
    anchor.rel = "noopener noreferrer";
    return anchor;
  } catch {
    return element("span", "无效链接");
  }
}
