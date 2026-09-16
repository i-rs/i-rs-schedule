import { getLang } from "@/lib/i18n";

/** 相对时间格式化(按当前语言):刚刚 / 5m ago / 3h ago / 2d ago。 */
export function timeAgo(iso: string): string {
  const seconds = Math.floor((Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return getLang() === "zh" ? "刚刚" : "just now";
  const minutes = Math.floor(seconds / 60);
  if (getLang() === "zh") {
    if (minutes < 60) return `${minutes} 分钟前`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours} 小时前`;
    return `${Math.floor(hours / 24)} 天前`;
  }
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

/** 未来时间格式化(按当前语言):now / in 5m / in 3h / 5 分钟后。 */
export function timeUntil(iso: string): string {
  const seconds = Math.floor((new Date(iso).getTime() - Date.now()) / 1000);
  const zh = getLang() === "zh";
  if (seconds <= 0) return zh ? "即将" : "now";
  const minutes = Math.floor(seconds / 60);
  if (zh) {
    if (minutes < 1) return "1 分钟内";
    if (minutes < 60) return `${minutes} 分钟后`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours} 小时后`;
    return `${Math.floor(hours / 24)} 天后`;
  }
  if (minutes < 1) return "in <1m";
  if (minutes < 60) return `in ${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `in ${hours}h`;
  return `in ${Math.floor(hours / 24)}d`;
}

/** 完整本地时间,用于 title 提示。 */
export function fullTime(iso: string): string {
  return new Date(iso).toLocaleString();
}

/** 按指定 IANA 时区格式化:"MM-DD HH:mm:ss"。 */
export function formatInTz(iso: string, tz: string): string {
  try {
    return new Date(iso).toLocaleString(undefined, {
      timeZone: tz,
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
  } catch {
    return iso;
  }
}
