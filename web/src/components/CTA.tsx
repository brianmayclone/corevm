import { motion } from 'framer-motion';
import { Download, Terminal, BookOpen } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface CTAProps {
  lang: Lang;
  onDownloadClick: () => void;
}

export default function CTA({ lang, onDownloadClick }: CTAProps) {
  return (
    <section id="get-started" className="relative py-24 md:py-32">
      <div className="pointer-events-none absolute inset-0">
        <div className="absolute inset-0 bg-gradient-to-t from-primary-500/10 via-transparent to-transparent" />
      </div>

      <div className="relative mx-auto max-w-4xl px-6 text-center">
        <motion.h2
          initial={{ opacity: 0, y: 10 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
        >
          {t(lang, 'cta_title')}
        </motion.h2>
        <motion.p
          initial={{ opacity: 0, y: 10 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.1 }}
          className="mx-auto mt-4 max-w-xl text-lg text-surface-400"
        >
          {t(lang, 'cta_subtitle')}
        </motion.p>

        {/* Action buttons */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.2 }}
          className="mt-10 flex flex-col items-center justify-center gap-4 sm:flex-row"
        >
          <button
            onClick={onDownloadClick}
            className="group inline-flex items-center gap-2 rounded-xl bg-primary-500 px-7 py-3.5 text-sm font-semibold text-white shadow-lg shadow-primary-500/25 transition-all hover:bg-primary-400 hover:shadow-xl hover:shadow-primary-500/30 cursor-pointer border-none"
          >
            <Download size={16} />
            {t(lang, 'cta_download')}
          </button>
          <a
            href="#"
            className="inline-flex items-center gap-2 rounded-xl border border-white/10 bg-white/5 px-7 py-3.5 text-sm font-semibold text-surface-300 transition-all hover:border-white/20 hover:bg-white/10 hover:text-white no-underline"
          >
            <BookOpen size={16} />
            {t(lang, 'cta_docs')}
          </a>
        </motion.div>

        {/* Build from source option */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.3 }}
          className="mx-auto mt-10 max-w-lg"
        >
          <p className="mb-3 text-xs font-medium uppercase tracking-wider text-surface-500">
            {t(lang, 'cta_build')}
          </p>
          <div className="overflow-hidden rounded-xl border border-white/10 bg-surface-900">
            <div className="flex items-center gap-2 border-b border-white/5 px-4 py-2.5">
              <Terminal size={14} className="text-surface-500" />
              <span className="text-xs font-medium text-surface-500">Terminal</span>
            </div>
            <div className="p-5 text-left">
              <code className="text-sm">
                <span className="text-surface-500">$</span>{' '}
                <span className="text-accent-400">git</span>{' '}
                <span className="text-surface-300">clone https://github.com/brianmayclone/corevm.git</span>
              </code>
              <br />
              <code className="text-sm">
                <span className="text-surface-500">$</span>{' '}
                <span className="text-accent-400">cd</span>{' '}
                <span className="text-surface-300">corevm && ./tools/build-iso.sh</span>
              </code>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
