import { motion } from 'framer-motion';
import { Monitor, Users, Download } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface VMManagerProps {
  lang: Lang;
  onDownloadClick: () => void;
}

export default function VMManager({ lang, onDownloadClick }: VMManagerProps) {
  return (
    <section id="vmmanager" className="relative py-24 md:py-32 overflow-hidden">
      {/* Background gradient */}
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-accent-400/[0.03] via-transparent to-transparent" />

      <div className="relative mx-auto max-w-7xl px-6">
        {/* Header */}
        <div className="mx-auto max-w-2xl text-center">
          <motion.span
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="inline-block rounded-full border border-accent-400/20 bg-accent-400/10 px-3 py-1 text-xs font-semibold uppercase tracking-wider text-accent-400"
          >
            {t(lang, 'vmm_badge')}
          </motion.span>
          <motion.h2
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="mt-4 text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
          >
            {t(lang, 'vmm_title')}
          </motion.h2>
          <motion.p
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="mt-4 text-lg text-surface-400"
          >
            {t(lang, 'vmm_subtitle')}
          </motion.p>
        </div>

        {/* Feature highlights */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.25 }}
          className="mx-auto mt-12 grid max-w-3xl gap-6 sm:grid-cols-2"
        >
          <div className="flex items-start gap-4 rounded-xl border border-white/5 bg-white/[0.02] p-6">
            <div className="rounded-lg border border-accent-400/20 bg-accent-400/10 p-2.5">
              <Monitor size={20} className="text-accent-400" />
            </div>
            <div>
              <h3 className="font-bold text-white">{t(lang, 'vmm_feat1_title')}</h3>
              <p className="mt-1 text-sm text-surface-400">{t(lang, 'vmm_feat1_desc')}</p>
            </div>
          </div>
          <div className="flex items-start gap-4 rounded-xl border border-white/5 bg-white/[0.02] p-6">
            <div className="rounded-lg border border-accent-400/20 bg-accent-400/10 p-2.5">
              <Users size={20} className="text-accent-400" />
            </div>
            <div>
              <h3 className="font-bold text-white">{t(lang, 'vmm_feat2_title')}</h3>
              <p className="mt-1 text-sm text-surface-400">{t(lang, 'vmm_feat2_desc')}</p>
            </div>
          </div>
        </motion.div>

        {/* Screenshots */}
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.3 }}
          className="mt-14 mx-auto max-w-2xl"
        >
          {/* Linux screenshot */}
          <div className="group overflow-hidden rounded-xl border border-white/10 bg-surface-900 shadow-2xl shadow-black/40 transition-all hover:border-accent-400/20">
            <div className="flex items-center gap-2 border-b border-white/5 bg-surface-800 px-4 py-2.5">
              <div className="flex gap-1.5">
                <div className="h-3 w-3 rounded-full bg-red-500/70" />
                <div className="h-3 w-3 rounded-full bg-yellow-500/70" />
                <div className="h-3 w-3 rounded-full bg-green-500/70" />
              </div>
              <span className="ml-2 text-xs text-surface-400">CoreVM VMManager — Linux</span>
            </div>
            <div className="overflow-hidden">
              <img
                src="/screenshots/vmmanager.png"
                alt="CoreVM VMManager on Linux"
                className="w-full object-cover transition-transform duration-500 group-hover:scale-[1.02]"
              />
            </div>
          </div>
        </motion.div>

        {/* CTA */}
        <motion.div
          initial={{ opacity: 0, y: 15 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.35 }}
          className="mt-12 text-center"
        >
          <button
            onClick={onDownloadClick}
            className="inline-flex items-center gap-2 rounded-lg bg-accent-400 px-6 py-3 text-sm font-semibold text-surface-950 transition-all hover:bg-accent-300 hover:shadow-lg hover:shadow-accent-400/25 cursor-pointer border-none"
          >
            <Download size={16} />
            {t(lang, 'vmm_cta')}
          </button>
          <p className="mt-3 text-sm text-surface-500">{t(lang, 'vmm_platforms')}</p>
        </motion.div>
      </div>
    </section>
  );
}
