import { defineConfig } from 'svedocs/config';
import { SITE_DESCRIPTION, SITE_NAME, SITE_URL } from './src/lib/seo';

export default defineConfig({
  site: {
    name: SITE_NAME,
    title: 'AIPass Documentation',
    description: SITE_DESCRIPTION,
    url: SITE_URL
  },
  content: {
    root: 'content',
    docs: 'content/docs',
    pages: 'content/pages'
  },
  build: {
    mode: 'static'
  },
  theme: {
    defaultMode: 'system',
    palette: {
      accent: '#2f66f6',
      neutral: 'slate'
    },
    fonts: {
      sans: 'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
      mono: 'ui-monospace, "SFMono-Regular", "Cascadia Code", monospace',
      display: 'ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    },
    radius: '10px',
    codeTheme: {
      light: 'github-light',
      dark: 'github-dark'
    },
    code: {
      lineNumbers: false,
      wrap: false,
      copyButton: true
    },
    brand: {
      label: 'AIPass',
      href: '/',
      logo: '/aipass.png',
      mark: false
    },
    nav: [
      { label: 'Docs', labelKey: 'aipass.nav.docs', href: '/docs' },
      { label: 'Download', labelKey: 'aipass.nav.download', href: '/#download' }
    ],
    social: [
      { label: 'GitHub', href: 'https://github.com/backrunner/aipass', external: true }
    ],
    footer: {
      text: 'AIPass is local-first credential tooling for AI workflows.',
      links: [
        { label: 'GitHub', href: 'https://github.com/backrunner/aipass', external: true },
        { label: 'Apache-2.0 License', href: 'https://github.com/backrunner/aipass/blob/main/LICENSE', external: true }
      ]
    }
  },
  search: {
    enabled: true,
    provider: 'local',
    scope: 'current'
  },
  ai: false,
  i18n: {
    defaultLocale: 'en',
    locales: [
      { code: 'en', label: 'English', hreflang: 'en', dir: 'ltr' },
      { code: 'zh', label: '中文', hreflang: 'zh-CN', dir: 'ltr' }
    ],
    messages: {
      zh: {
        'nav.primary': '主导航',
        'nav.docs': '文档',
        'nav.documentation': '文档导航',
        'nav.footer': '页脚',
        'nav.social': '社交链接',
        'nav.mobile.open': '打开菜单',
        'nav.mobile.close': '关闭菜单',
        'nav.skipToContent': '跳到正文',
        'scope.locale': '语言',
        'scope.localeOptions': '语言选项',
        'scope.langShort': '语言',
        'search.trigger': '搜索',
        'search.dialog': '搜索文档',
        'search.query': '搜索关键词',
        'search.placeholder': '搜索文档',
        'search.results': '搜索结果',
        'search.loading': '正在搜索…',
        'search.loadingIndex': '正在加载搜索索引…',
        'search.indexError': '无法加载搜索索引。',
        'search.empty': '没有匹配的文档。',
        'toc.label': '本页内容',
        'heading.anchor': '链接到本节',
        'article.kind.doc': '文档',
        'article.kind.page': '页面',
        'article.breadcrumb': '路径导航',
        'article.updated': '更新于 {date}',
        'article.edit': '编辑此页',
        'article.previous': '上一页',
        'article.next': '下一页',
        'code.copy': '复制代码',
        'code.copied': '已复制',
        'theme.switch': '切换到{mode}主题',
        'theme.light': '浅色',
        'theme.dark': '深色',
        'tools.label': '页面工具',
        'tools.backToTop': '回到顶部',
        'footer.text': '面向 AI 工作流的本地优先凭据工具。',
        'aipass.nav.docs': '文档',
        'aipass.nav.download': '下载',
        'error.notFound.title': '页面未找到',
        'error.notFound.description': '找不到与当前地址对应的页面。',
        'error.backToDocs': '返回文档'
      }
    }
  },
  checks: {
    translations: true,
    assets: true,
    externalLinks: false
  },
  cloudflare: {
    compatibilityDate: '2026-07-15'
  },
  source: {
    editBaseUrl: 'https://github.com/backrunner/aipass/edit/main/apps/web'
  },
  seo: {
    sitemap: true,
    robots: true,
    defaultAuthor: 'AIPass contributors',
    ogImage: {
      template: 'default',
      format: 'png',
      outDir: 'static/og',
      renderer: 'svg'
    }
  }
});
