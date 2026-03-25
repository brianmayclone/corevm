import { motion } from 'framer-motion';
import { Disc, Rocket, Globe, Check, Server, Network } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface ApplianceProps {
  lang: Lang;
}

const steps = [
  { icon: Disc, title: 'appliance_step1_title', desc: 'appliance_step1_desc' },
  { icon: Rocket, title: 'appliance_step2_title', desc: 'appliance_step2_desc' },
  { icon: Globe, title: 'appliance_step3_title', desc: 'appliance_step3_desc' },
] as const;

const includes = [
  'appliance_kernel',
  'appliance_installer',
  'appliance_dcui',
  'appliance_firewall',
  'appliance_tls',
  'appliance_updates',
] as const;

export default function Appliance({ lang }: ApplianceProps) {
  return (
    <section id="appliance" className="relative py-24 md:py-32">
      {/* Background */}
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-primary-500/[0.03] via-transparent to-transparent" />

      <div className="relative mx-auto max-w-7xl px-6">
        {/* Header */}
        <div className="mx-auto max-w-2xl text-center">
          <motion.span
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="inline-block rounded-full border border-primary-500/20 bg-primary-500/10 px-3 py-1 text-xs font-semibold uppercase tracking-wider text-primary-300"
          >
            {t(lang, 'appliance_badge')}
          </motion.span>
          <motion.h2
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="mt-4 text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
          >
            {t(lang, 'appliance_title')}
          </motion.h2>
          <motion.p
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="mt-4 text-lg text-surface-400"
          >
            {t(lang, 'appliance_subtitle')}
          </motion.p>
        </div>

        {/* 3-Step Process */}
        <div className="mt-16 grid gap-8 md:grid-cols-3">
          {steps.map((step, i) => {
            const Icon = step.icon;
            return (
              <motion.div
                key={step.title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: i * 0.1 }}
                className="relative"
              >
                {/* Connector line (desktop) */}
                {i < steps.length - 1 && (
                  <div className="pointer-events-none absolute right-0 top-10 hidden h-px w-8 translate-x-full bg-gradient-to-r from-primary-500/30 to-transparent md:block" />
                )}

                <div className="rounded-2xl border border-white/5 bg-white/[0.02] p-8 transition-all hover:border-primary-500/20 hover:bg-white/[0.04]">
                  {/* Step number */}
                  <div className="mb-4 flex items-center gap-3">
                    <div className="flex h-10 w-10 items-center justify-center rounded-xl border border-primary-500/20 bg-primary-500/10">
                      <Icon size={20} className="text-primary-400" />
                    </div>
                    <span className="text-xs font-bold uppercase tracking-widest text-surface-500">Step {i + 1}</span>
                  </div>
                  <h3 className="text-lg font-bold text-white">{t(lang, step.title)}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-surface-400">{t(lang, step.desc)}</p>
                </div>
              </motion.div>
            );
          })}
        </div>

        {/* What's included + Deployment Modes */}
        <div className="mt-16 grid gap-8 lg:grid-cols-3">
          {/* What's included */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="rounded-2xl border border-white/5 bg-white/[0.02] p-8"
          >
            <h3 className="text-sm font-semibold uppercase tracking-wider text-surface-300 mb-5">
              {t(lang, 'appliance_includes')}
            </h3>
            <ul className="space-y-3">
              {includes.map((key) => (
                <li key={key} className="flex items-center gap-3">
                  <div className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-primary-500/20">
                    <Check size={12} className="text-primary-400" />
                  </div>
                  <span className="text-sm text-surface-300">{t(lang, key)}</span>
                </li>
              ))}
            </ul>
          </motion.div>

          {/* Standalone */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="group relative overflow-hidden rounded-2xl border border-white/5 bg-white/[0.02] p-8 transition-all hover:border-primary-500/20"
          >
            <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-primary-500/50 to-transparent" />
            <div className="mb-4 inline-flex rounded-xl border border-primary-500/20 bg-primary-500/10 p-3">
              <Server size={22} className="text-primary-400" />
            </div>
            <h3 className="text-lg font-bold text-white">{t(lang, 'appliance_standalone')}</h3>
            <p className="mt-2 text-sm leading-relaxed text-surface-400">{t(lang, 'appliance_standalone_desc')}</p>

            <div className="mt-5 rounded-lg bg-surface-900/50 p-3 font-mono text-xs text-surface-400">
              <span className="text-primary-400">Port 8443</span> &middot; Web UI + REST API
            </div>
          </motion.div>

          {/* Cluster Controller */}
          <motion.div
            initial={{ opacity: 0, y: 20 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="group relative overflow-hidden rounded-2xl border border-white/5 bg-white/[0.02] p-8 transition-all hover:border-accent-400/20"
          >
            <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-accent-400/50 to-transparent" />
            <div className="mb-4 inline-flex rounded-xl border border-accent-400/20 bg-accent-400/10 p-3">
              <Network size={22} className="text-accent-400" />
            </div>
            <h3 className="text-lg font-bold text-white">{t(lang, 'appliance_cluster_mode')}</h3>
            <p className="mt-2 text-sm leading-relaxed text-surface-400">{t(lang, 'appliance_cluster_desc')}</p>

            <div className="mt-5 rounded-lg bg-surface-900/50 p-3 font-mono text-xs text-surface-400">
              <span className="text-accent-400">Port 9443</span> &middot; DRS &middot; HA &middot; SDN &middot; Migration
            </div>
          </motion.div>
        </div>
      </div>
    </section>
  );
}
