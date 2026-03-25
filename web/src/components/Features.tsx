import { motion } from 'framer-motion';
import { Zap, Monitor, Code2, HardDrive, TerminalSquare, ShieldCheck } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface FeaturesProps {
  lang: Lang;
}

const iconMap = [Zap, Monitor, Code2, HardDrive, TerminalSquare, ShieldCheck];

const featureKeys = [
  { title: 'feat_hw_title', desc: 'feat_hw_desc' },
  { title: 'feat_web_title', desc: 'feat_web_desc' },
  { title: 'feat_api_title', desc: 'feat_api_desc' },
  { title: 'feat_devices_title', desc: 'feat_devices_desc' },
  { title: 'feat_dcui_title', desc: 'feat_dcui_desc' },
  { title: 'feat_security_title', desc: 'feat_security_desc' },
] as const;

export default function Features({ lang }: FeaturesProps) {
  return (
    <section id="features" className="relative py-24 md:py-32">
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-transparent via-primary-500/[0.02] to-transparent" />

      <div className="relative mx-auto max-w-7xl px-6">
        {/* Header */}
        <div className="mx-auto max-w-2xl text-center">
          <motion.span
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="inline-block rounded-full border border-primary-500/20 bg-primary-500/10 px-3 py-1 text-xs font-semibold uppercase tracking-wider text-primary-300"
          >
            {t(lang, 'features_badge')}
          </motion.span>
          <motion.h2
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="mt-4 text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
          >
            {t(lang, 'features_title')}
          </motion.h2>
          <motion.p
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="mt-4 text-lg text-surface-400"
          >
            {t(lang, 'features_subtitle')}
          </motion.p>
        </div>

        {/* Grid */}
        <div className="mt-16 grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {featureKeys.map((feature, i) => {
            const Icon = iconMap[i];
            return (
              <motion.div
                key={feature.title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: i * 0.08 }}
                className="group relative rounded-2xl border border-white/5 bg-white/[0.02] p-8 transition-all hover:border-primary-500/20 hover:bg-white/[0.04]"
              >
                <div className="pointer-events-none absolute inset-0 rounded-2xl opacity-0 transition-opacity group-hover:opacity-100 bg-gradient-to-br from-primary-500/5 to-transparent" />

                <div className="relative">
                  <div className="mb-5 inline-flex rounded-xl border border-primary-500/20 bg-primary-500/10 p-3">
                    <Icon size={22} className="text-primary-400" />
                  </div>
                  <h3 className="text-lg font-bold text-white">
                    {t(lang, feature.title)}
                  </h3>
                  <p className="mt-2 text-sm leading-relaxed text-surface-400">
                    {t(lang, feature.desc)}
                  </p>
                </div>
              </motion.div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
