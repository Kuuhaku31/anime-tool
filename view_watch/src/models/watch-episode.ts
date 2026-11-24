
// view_watch/src/models/watch-episode.ts
// 描述观看剧集视图, 筛选条件和编辑草稿.

/** 观看剧集视图的一行, bigint 保留为十进制文本. */
export interface WatchEpisodeRow {
  /** Bangumi 剧集 ID. */ bgm_epi_id: number | null;
  /** Bangumi 动画 ID. */ bgm_ani_id: number | null;
  /** 弹弹播放动画 ID. */ ddp_ani_id: string | null;
  /** 蜜柑订阅 ID. */ mikan_rss_id: number | null;
  /** 蜜柑订阅标题. */ mikan_rss_title: string | null;
  /** Bangumi 动画标题. */ bgm_ani_title: string | null;
  /** 剧集序号. */ bgm_epi_ep: string | null;
  /** 剧集标题. */ bgm_epi_title: string | null;
  /** 单集放送日期, 不含时区. */ bgm_epi_airdate: string | null;
  /** ISO 星期, 1 为星期一, 7 为星期日. */ bgm_epi_weekday: number | null;
  /** 观看时间, 为空表示未观看. */ watched_at: string | null;
  /** 观看评分. */ rating: number | null;
}

/** 剧集页面查询参数. */
export interface EpisodeQuery {
  /** 搜索内容. */ search: string;
  /** 观看状态筛选, aired-no 表示截至今天放送且未观看. */ watched: "all" | "yes" | "no" | "aired-no";
  /** 评分筛选, 空字符串表示全部. */ rating: string;
  /** 排序字段. */ sort: "bgm_epi_id" | "bgm_ani_id" | "bgm_ani_title" | "bgm_epi_ep" | "bgm_epi_airdate" | "watched_at" | "rating";
  /** 排序方向. */ direction: "asc" | "desc";
  /** 从零开始的页码. */ page: number;
  /** 每页记录数. */ pageSize: number;
}

/** 单页结果和精确总数. */
export interface EpisodePage {
  /** 当前页视图数据. */ items: WatchEpisodeRow[];
  /** 满足筛选条件的总行数. */ total: number;
}

/** 按剧集 ID 保存的编辑草稿. */
export interface EpisodeDraft {
  /** 原始观看时间, 用于保留未修改时间的精度和时区. */ originalTime: string | null;
  /** 输入的本地日期时间. */ time: string;
  /** 输入的评分文本. */ rating: string;
  /** 时间是否被编辑. */ timeChanged: boolean;
  /** 是否存在未保存的字段修改. */ dirty: boolean;
  /** 操作是否正在进行. */ saving: boolean;
  /** 该行的错误信息. */ error: string | null;
}
