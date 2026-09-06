import { defineConfig } from "vitepress";
import { withMermaid } from "vitepress-plugin-mermaid";

const zhNav = [
  { text: "官网", link: "/" },
  { text: "文档", link: "/quickstart" },
  { text: "GitHub", link: "https://github.com/myki-jim/frp-sh" },
];

const zhSidebar = [
  {
    text: "入门",
    items: [
      { text: "简介", link: "/intro" },
      { text: "名字的由来", link: "/name" },
      { text: "快速开始", link: "/quickstart" },
      { text: "安装与构建", link: "/install" },
      { text: "部署信令服务器", link: "/server" },
    ],
  },
  {
    text: "使用",
    items: [
      { text: "命令参考", link: "/cli" },
      { text: "配置文件", link: "/config" },
      { text: "高级用法", link: "/advanced" },
      { text: "故障排查 FAQ", link: "/faq" },
    ],
  },
  {
    text: "原理与开发",
    items: [
      { text: "网络原理", link: "/architecture" },
      { text: "协议规范", link: "/protocol" },
      { text: "版本策略", link: "/versioning" },
      { text: "开发与测试", link: "/develop" },
      { text: "路线图", link: "/roadmap" },
    ],
  },
];

const enNav = [
  { text: "Home", link: "/" },
  { text: "Guide", link: "/en/quickstart" },
  { text: "GitHub", link: "https://github.com/myki-jim/frp-sh" },
];

const enSidebar = [
  {
    text: "Getting Started",
    items: [
      { text: "Introduction", link: "/en/intro" },
      { text: "About the Name", link: "/en/name" },
      { text: "Quickstart", link: "/en/quickstart" },
      { text: "Installation", link: "/en/install" },
      { text: "Deploy the Server", link: "/en/server" },
    ],
  },
  {
    text: "Usage",
    items: [
      { text: "CLI Reference", link: "/en/cli" },
      { text: "Configuration", link: "/en/config" },
      { text: "Advanced Usage", link: "/en/advanced" },
      { text: "Troubleshooting FAQ", link: "/en/faq" },
    ],
  },
  {
    text: "Internals",
    items: [
      { text: "Architecture", link: "/en/architecture" },
      { text: "Protocol Spec", link: "/en/protocol" },
      { text: "Versioning", link: "/en/versioning" },
      { text: "Development & Testing", link: "/en/develop" },
      { text: "Roadmap", link: "/en/roadmap" },
    ],
  },
];

export default withMermaid({
  transformHtml(code, _id, { pageData }) {
    return pageData.frontmatter.cinematic
      ? code.replace('<html lang="zh-CN"', '<html lang="en"')
      : code;
  },
  lang: "zh-CN",
  title: "frp-sh",
  description: "社交化 P2P 打洞工具使用手册 · Social P2P tunnel documentation",
  cleanUrls: true,
  lastUpdated: true,
  locales: {
    root: {
      label: "中文",
      lang: "zh-CN",
      title: "frp-sh",
      description: "社交化 P2P 打洞工具使用手册",
      themeConfig: {
        nav: zhNav,
        sidebarMenuLabel: "目录",
        returnToTopLabel: "返回顶部",
        skipToContentLabel: "跳到正文",
        sidebar: zhSidebar,
        outline: { label: "本页目录", level: [2, 3] },
        docFooter: { prev: "上一页", next: "下一页" },
        lastUpdated: {
          text: "最后更新",
          formatOptions: { dateStyle: "short", timeStyle: "short" },
        },
        search: {
          provider: "local",
          options: {
            translations: {
              button: { buttonText: "搜索文档", buttonAriaLabel: "搜索" },
              modal: {
                noResultsText: "未找到结果",
                resetButtonTitle: "清除",
                footer: {
                  selectText: "选择",
                  navigateText: "切换",
                  closeText: "关闭",
                },
              },
            },
          },
        },
      },
    },
    en: {
      label: "English",
      lang: "en",
      title: "frp-sh",
      description: "Social P2P tunnel documentation",
      themeConfig: {
        nav: enNav,
        sidebar: enSidebar,
        outline: { label: "On this page", level: [2, 3] },
        docFooter: { prev: "Previous", next: "Next" },
        editLink: {
          pattern:
            "https://github.com/myki-jim/frp-sh/edit/main/web/docs/:path",
          text: "Edit this page on GitHub",
        },
      },
    },
  },
  themeConfig: {
    search: { provider: "local" },
    siteTitle: "frp.sh",
    socialLinks: [
      { icon: "github", link: "https://github.com/myki-jim/frp-sh" },
    ],
    editLink: {
      pattern: "https://github.com/myki-jim/frp-sh/edit/main/web/docs/:path",
      text: "在 GitHub 上编辑此页",
    },
    footer: {
      message: "frp-sh · 社交化 P2P 打洞工具",
      copyright: "MIT License",
    },
  },
  mermaid: { theme: "default" },
});
