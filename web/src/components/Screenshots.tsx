import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface ScreenshotsProps {
  lang: Lang;
}

const screenshots = [
  { key: 'screenshot_dashboard', src: '/screenshots/dashboard.png' },
  { key: 'screenshot_vms', src: '/screenshots/vm.png' },
  { key: 'screenshot_settings', src: '/screenshots/vmsettings.png' },
  { key: 'screenshot_storage', src: '/screenshots/storage.png' },
  { key: 'screenshot_network', src: '/screenshots/network.png' },
] as const;

export default function Screenshots({ lang }: ScreenshotsProps) {
  const [active, setActive] = useState(0);

  return (
    <section id="screenshots" className="relative py-24 md:py-32">
      <div className="mx-auto max-w-7xl px-6">
        {/* Header */}
        <div className="mx-auto max-w-2xl text-center">
          <motion.span
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="inline-block rounded-full border border-accent-400/20 bg-accent-400/10 px-3 py-1 text-xs font-semibold uppercase tracking-wider text-accent-400"
          >
            {t(lang, 'screenshots_badge')}
          </motion.span>
          <motion.h2
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="mt-4 text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
          >
            {t(lang, 'screenshots_title')}
          </motion.h2>
          <motion.p
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="mt-4 text-lg text-surface-400"
          >
            {t(lang, 'screenshots_subtitle')}
          </motion.p>
        </div>

        {/* Tab navigation */}
        <div className="mt-12 flex flex-wrap justify-center gap-2">
          {screenshots.map((s, i) => (
            <button
              key={s.key}
              onClick={() => setActive(i)}
              className={`cursor-pointer rounded-lg border px-4 py-2 text-sm font-medium transition-all ${
                active === i
                  ? 'border-primary-500/50 bg-primary-500/15 text-primary-300'
                  : 'border-white/5 bg-white/[0.02] text-surface-400 hover:border-white/10 hover:text-white'
              }`}
            >
              {t(lang, s.key)}
            </button>
          ))}
        </div>

        {/* Screenshot display */}
        <motion.div
          initial={{ opacity: 0, y: 30 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          transition={{ delay: 0.2 }}
          className="relative mt-10"
        >
          {/* Browser chrome mockup */}
          <div className="overflow-hidden rounded-xl border border-white/10 bg-surface-900 shadow-2xl shadow-black/50">
            {/* Title bar */}
            <div className="flex items-center gap-2 border-b border-white/5 bg-surface-800 px-4 py-3">
              <div className="flex gap-1.5">
                <div className="h-3 w-3 rounded-full bg-red-500/70" />
                <div className="h-3 w-3 rounded-full bg-yellow-500/70" />
                <div className="h-3 w-3 rounded-full bg-green-500/70" />
              </div>
              <div className="ml-3 flex-1">
                <div className="mx-auto max-w-sm rounded-md bg-surface-700 px-3 py-1 text-center text-xs text-surface-400">
                  https://corevm.local:8443
                </div>
              </div>
            </div>

            {/* Screenshot */}
            <div className="relative aspect-[16/10] overflow-hidden bg-surface-900">
              <AnimatePresence mode="wait">
                <motion.img
                  key={active}
                  src={screenshots[active].src}
                  alt={t(lang, screenshots[active].key)}
                  initial={{ opacity: 0, scale: 1.02 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.98 }}
                  transition={{ duration: 0.3 }}
                  className="h-full w-full object-cover object-top"
                />
              </AnimatePresence>
            </div>
          </div>

          {/* Glow */}
          <div className="pointer-events-none absolute -inset-4 -z-10 rounded-2xl bg-gradient-to-b from-primary-500/10 via-transparent to-accent-400/10 blur-2xl" />
        </motion.div>

        {/* Mobile screenshots */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="mt-16 grid grid-cols-1 gap-6 sm:grid-cols-3"
        >
          {[
            { src: '/screenshots/mobile_vms.png', label: t(lang, 'screenshot_mobile') + ' — VMs' },
            { src: '/screenshots/mobile_details.png', label: t(lang, 'screenshot_mobile') + ' — Details' },
            { src: '/screenshots/mobile_console.png', label: t(lang, 'screenshot_mobile') + ' — Console' },
          ].map((item) => (
            <div
              key={item.src}
              className="group overflow-hidden rounded-xl border border-white/5 bg-white/[0.02] transition-all hover:border-white/10"
            >
              <div className="overflow-hidden">
                <img
                  src={item.src}
                  alt={item.label}
                  className="w-full object-cover transition-transform duration-500 group-hover:scale-105"
                />
              </div>
              <div className="px-4 py-3">
                <p className="text-xs font-medium text-surface-400">{item.label}</p>
              </div>
            </div>
          ))}
        </motion.div>
      </div>
    </section>
  );
}
