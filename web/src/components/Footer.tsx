import type { Lang } from '../i18n';
import { t } from '../i18n';

import type { LegalPage } from './LegalModal';

interface FooterProps {
  lang: Lang;
  onLegalClick: (page: LegalPage) => void;
}

export default function Footer({ lang, onLegalClick }: FooterProps) {
  return (
    <footer className="border-t border-white/5 bg-surface-950">
      <div className="mx-auto max-w-7xl px-6 py-16">
        <div className="grid gap-12 md:grid-cols-4">
          {/* Brand */}
          <div className="md:col-span-2">
            <div className="flex items-center gap-3 mb-4">
              <img src="/icons/logo.png" alt="CoreVM" className="h-9 w-9 rounded-lg" />
              <span className="text-xl font-bold text-white tracking-tight">CoreVM</span>
            </div>
            <p className="max-w-sm text-sm leading-relaxed text-surface-400">
              {t(lang, 'footer_desc')}
            </p>
          </div>

          {/* Product */}
          <div>
            <h4 className="mb-4 text-sm font-semibold uppercase tracking-wider text-surface-300">
              {t(lang, 'footer_product')}
            </h4>
            <ul className="space-y-3 list-none p-0 m-0">
              <li><a href="#features" className="text-sm text-surface-400 hover:text-white transition-colors no-underline">{t(lang, 'nav_features')}</a></li>
              <li><a href="#screenshots" className="text-sm text-surface-400 hover:text-white transition-colors no-underline">{t(lang, 'nav_screenshots')}</a></li>
              <li><a href="#architecture" className="text-sm text-surface-400 hover:text-white transition-colors no-underline">{t(lang, 'nav_architecture')}</a></li>
              <li><a href="#cluster" className="text-sm text-surface-400 hover:text-white transition-colors no-underline">{t(lang, 'nav_cluster')}</a></li>
            </ul>
          </div>

          {/* Resources */}
          <div>
            <h4 className="mb-4 text-sm font-semibold uppercase tracking-wider text-surface-300">
              {t(lang, 'footer_resources')}
            </h4>
            <ul className="space-y-3 list-none p-0 m-0">
              <li><a href="#" className="text-sm text-surface-400 hover:text-white transition-colors no-underline">{t(lang, 'footer_documentation')}</a></li>
              <li><a href="#" className="text-sm text-surface-400 hover:text-white transition-colors no-underline">{t(lang, 'footer_api_reference')}</a></li>
              <li><a href="https://github.com/brianmayclone/corevm" className="text-sm text-surface-400 hover:text-white transition-colors no-underline">{t(lang, 'footer_github')}</a></li>
            </ul>
          </div>
        </div>

        {/* Bottom */}
        <div className="mt-16 flex flex-col items-center justify-between gap-4 border-t border-white/5 pt-8 sm:flex-row">
          <p className="text-xs text-surface-500">
            &copy; {new Date().getFullYear()} CoreVM. {t(lang, 'footer_rights')}
          </p>
          <div className="flex items-center gap-4">
            <button
              onClick={() => onLegalClick('imprint')}
              className="text-xs text-surface-500 hover:text-white transition-colors cursor-pointer bg-transparent border-none p-0"
            >
              {t(lang, 'legal_imprint')}
            </button>
            <button
              onClick={() => onLegalClick('privacy')}
              className="text-xs text-surface-500 hover:text-white transition-colors cursor-pointer bg-transparent border-none p-0"
            >
              {t(lang, 'legal_privacy')}
            </button>
          </div>
        </div>
      </div>
    </footer>
  );
}
