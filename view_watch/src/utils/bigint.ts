
// view_watch/src/utils/bigint.ts
// 校验数据库剧集 ID 并保留 bigint 精度.

/** 将用户输入转换为合法的十进制 ID 或空值. */
export function parseEpisodeId(input: string): string | null {
  const value = input.trim();
  if (!value) return null;
  if (!/^[+-]?\d+$/.test(value)) throw new Error("请输入整数, -1 或留空.");
  const number = BigInt(value);
  if (number < -1n || number > 9223372036854775807n) {
    throw new Error("剧集 ID 必须在 -1 到 9223372036854775807 之间.");
  }
  return number.toString();
}
