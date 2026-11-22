
// view_watch/src/models/watch.ts
// 描述数据库视图, 筛选条件和分页响应.

/** 表示 view_watch 中的一行原始数据. */
export interface WatchRow {
  /** 种子哈希. */ mikan_torrent_hash_v1: string | null;
  /** 文件 SHA-256. */ file_sha256: string | null;
  /** RSS 标识. */ mikan_rss_id: number | null;
  /** RSS 标题. */ mikan_rss_title: string | null;
  /** 种子字节数的十进制文本. */ mikan_torrent_file_size: string | null;
  /** 格式化文件大小. */ file_size: string | null;
  /** 种子标题. */ mikan_torrent_title: string | null;
  /** 种子描述. */ mikan_torrent_description: string | null;
  /** 种子下载地址. */ mikan_torrent_download_url: string | null;
  /** 种子页面地址. */ mikan_torrent_page_url: string | null;
  /** 种子发布时间. */ mikan_torrent_pub_at: string | null;
  /** 种子更新时间. */ mikan_torrent_update_at: string | null;
  /** qBittorrent 存在状态. */ in_qb: number | null;
  /** 文件绝对路径. */ file_absolute_path: string | null;
  /** 文件目录. */ file_directory: string | null;
  /** 文件名. */ file_filename: string | null;
  /** 文件扩展名. */ file_extension: string | null;
  /** 文件打开地址. */ file_url: string | null;
  /** 弹弹播放剧集 ID 的十进制文本. */ ddp_epi_id: string | null;
  /** 弹弹播放动画 ID 的十进制文本. */ ddp_ani_id: string | null;
  /** Bangumi 动画 ID. */ bgm_ani_id: number | null;
  /** 弹弹播放动画标题. */ dandanplay_anime_title: string | null;
  /** 弹弹播放剧集序号. */ dandanplay_episode_ep: string | null;
  /** 弹弹播放剧集标题. */ dandanplay_episode_title: string | null;
  /** 最后弹幕更新时间. */ dandanplay_episode_danmaku_update_at: string | null;
  /** Bangumi 动画标题. */ bangumi_anime_title: string | null;
}

/** 可选的服务端排序字段. */
export type SortField = "mikan_rss_id" | "mikan_torrent_pub_at" | "mikan_torrent_update_at"
  | "file_filename" | "mikan_torrent_file_size" | "ddp_epi_id" | "dandanplay_episode_danmaku_update_at";

/** 描述当前列表筛选和分页. */
export interface WatchQuery {
  /** 搜索文本. */ search: string;
  /** 蜜柑 RSS ID, 空字符串表示不筛选. */ mikanRssId: string;
  /** 剧集关联状态. */ ddp: "all" | "null" | "unmatchable" | "assigned";
  /** 下载器状态. */ qb: "all" | "yes" | "no";
  /** 是否拥有文件. */ editable: "all" | "yes" | "no";
  /** 排序字段. */ sort: SortField;
  /** 排序方向. */ direction: "asc" | "desc";
  /** 从零开始的页码. */ page: number;
  /** 每页行数. */ pageSize: number;
}

/** 描述单页查询结果. */
export interface WatchPage {
  /** 当前页原始行. */ items: WatchRow[];
  /** 满足筛选的总行数. */ total: number;
}

/** 描述单个文件的编辑状态. */
export interface EditorState {
  /** 输入草稿. */ draft: string;
  /** 是否正在写入或刷新. */ saving: boolean;
  /** 局部错误消息. */ error: string | null;
}
