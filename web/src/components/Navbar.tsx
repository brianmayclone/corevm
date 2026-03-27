import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Menu, X, Download } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface NavbarProps {
  lang: Lang;
  onLangChange: (lang: Lang) => void;
  onDownloadClick: () => void;
}

export default function Navbar({ lang, onLangChange, onDownloadClick }: NavbarProps) {
  const [mobileOpen, setMobileOpen] = useState(false);

  const links = [
    { href: '#appliance', label: t(lang, 'nav_appliance') },
    { href: '#features', label: t(lang, 'nav_features') },
    { href: '#screenshots', label: t(lang, 'nav_screenshots') },
    { href: '#cluster', label: t(lang, 'nav_cluster') },
    { href: '#architecture', label: t(lang, 'nav_architecture') },
    { href: '#vmmanager', label: 'VMManager' },
  ];

  return (
    <nav className="fixed top-0 left-0 right-0 z-50 border-b border-white/5 bg-surface-950/80 backdrop-blur-xl">
      <div className="mx-auto flex max-w-7xl items-center justify-between px-6 py-4">
        {/* Logo */}
        <a href="#" className="flex items-center gap-3 text-white no-underline">
          <img src="/icons/logo.png" alt="CoreVM" className="h-9 w-9 rounded-lg" />
          <span className="text-xl font-bold tracking-tight">CoreVM</span>
        </a>

        {/* Desktop links */}
        <div className="hidden items-center gap-8 lg:flex">
          {links.map((link) => (
            <a
              key={link.href}
              href={link.href}
              className="text-sm font-medium text-surface-400 transition-colors hover:text-white no-underline"
            >
              {link.label}
            </a>
          ))}
        </div>

        {/* Right side */}
        <div className="flex items-center gap-3">
          {/* Language toggle */}
          <button
            onClick={() => onLangChange(lang === 'en' ? 'de' : 'en')}
            className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-xs font-semibold text-surface-300 transition-all hover:border-primary-500/50 hover:text-white cursor-pointer"
          >
            {lang === 'en' ? 'DE' : 'EN'}
          </button>

          {/* CTA */}
          <button
            onClick={onDownloadClick}
            className="hidden rounded-lg bg-primary-500 px-4 py-2 text-sm font-semibold text-white transition-all hover:bg-primary-400 hover:shadow-lg hover:shadow-primary-500/25 cursor-pointer border-none sm:inline-flex sm:items-center sm:gap-2"
          >
            <Download size={14} />
            {t(lang, 'nav_get_started')}
          </button>

          {/* Mobile menu button */}
          <button
            onClick={() => setMobileOpen(!mobileOpen)}
            className="text-surface-400 hover:text-white lg:hidden cursor-pointer bg-transparent border-none"
          >
            {mobileOpen ? <X size={24} /> : <Menu size={24} />}
          </button>
        </div>
      </div>

      {/* Mobile menu */}
      <AnimatePresence>
        {mobileOpen && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            className="overflow-hidden border-t border-white/5 bg-surface-950/95 backdrop-blur-xl lg:hidden"
          >
            <div className="flex flex-col gap-1 px-6 py-4">
              {links.map((link) => (
                <a
                  key={link.href}
                  href={link.href}
                  onClick={() => setMobileOpen(false)}
                  className="rounded-lg px-4 py-3 text-sm font-medium text-surface-300 transition-colors hover:bg-white/5 hover:text-white no-underline"
                >
                  {link.label}
                </a>
              ))}
              <button
                onClick={() => { setMobileOpen(false); onDownloadClick(); }}
                className="mt-2 inline-flex items-center justify-center gap-2 rounded-lg bg-primary-500 px-4 py-3 text-center text-sm font-semibold text-white cursor-pointer border-none"
              >
                <Download size={14} />
                {t(lang, 'nav_get_started')}
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </nav>
  );
}
