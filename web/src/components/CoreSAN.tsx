import { motion } from 'framer-motion';
import {
  HardDrive,
  ShieldCheck,
  RefreshCw,
  Layers,
  Activity,
  Database,
  Copy,
  Network,
  SearchCheck,
  Gauge,
} from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface CoreSANProps {
  lang: Lang;
}

const highlights = [
  { icon: Copy, title: 'san_replication', desc: 'san_replication_desc' },
  { icon: ShieldCheck, title: 'san_selfhealing', desc: 'san_selfhealing_desc' },
  { icon: Layers, title: 'san_fuse', desc: 'san_fuse_desc' },
  { icon: Network, title: 'san_decentralized', desc: 'san_decentralized_desc' },
] as const;

const details = [
  { icon: Database, title: 'san_raid', desc: 'san_raid_desc' },
  { icon: Activity, title: 'san_quorum', desc: 'san_quorum_desc' },
  { icon: SearchCheck, title: 'san_integrity', desc: 'san_integrity_desc' },
  { icon: Gauge, title: 'san_benchmark', desc: 'san_benchmark_desc' },
  { icon: RefreshCw, title: 'san_rebalancer', desc: 'san_rebalancer_desc' },
  { icon: HardDrive, title: 'san_hotplug', desc: 'san_hotplug_desc' },
] as const;

export default function CoreSAN({ lang }: CoreSANProps) {
  return (
    <section id="coresan" className="relative py-24 md:py-32">
      {/* Background gradient */}
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-transparent via-primary-500/[0.03] to-transparent" />

      <div className="relative mx-auto max-w-7xl px-6">
        {/* Header */}
        <div className="mx-auto max-w-2xl text-center">
          <motion.span
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="inline-block rounded-full border border-primary-500/20 bg-primary-500/10 px-3 py-1 text-xs font-semibold uppercase tracking-wider text-primary-300"
          >
            {t(lang, 'san_badge')}
          </motion.span>
          <motion.h2
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="mt-4 text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
          >
            {t(lang, 'san_title')}
          </motion.h2>
          <motion.p
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="mt-4 text-lg text-surface-400"
          >
            {t(lang, 'san_subtitle')}
          </motion.p>
        </div>

        {/* Top 4 highlight cards */}
        <div className="mt-16 grid gap-6 sm:grid-cols-2">
          {highlights.map((feature, i) => {
            const Icon = feature.icon;
            return (
              <motion.div
                key={feature.title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: i * 0.08 }}
                className="group relative overflow-hidden rounded-2xl border border-white/5 bg-white/[0.02] p-8 transition-all hover:border-primary-500/20 hover:bg-white/[0.04]"
              >
                <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-primary-500/40 to-transparent" />
                <div className="pointer-events-none absolute inset-0 rounded-2xl opacity-0 transition-opacity group-hover:opacity-100 bg-gradient-to-br from-primary-500/5 to-transparent" />

                <div className="relative">
                  <div className="mb-5 inline-flex rounded-xl border border-primary-500/20 bg-primary-500/10 p-3">
                    <Icon size={22} className="text-primary-400" />
                  </div>
                  <h3 className="text-lg font-bold text-white">{t(lang, feature.title)}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-surface-400">{t(lang, feature.desc)}</p>
                </div>
              </motion.div>
            );
          })}
        </div>

        {/* Bottom 6 compact cards */}
        <div className="mt-8 grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
          {details.map((feature, i) => {
            const Icon = feature.icon;
            return (
              <motion.div
                key={feature.title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: 0.3 + i * 0.08 }}
                className="rounded-2xl border border-white/5 bg-white/[0.02] p-6 transition-all hover:border-white/10"
              >
                <div className="mb-3 inline-flex rounded-lg border border-white/10 bg-white/5 p-2">
                  <Icon size={16} className="text-surface-400" />
                </div>
                <h3 className="text-sm font-bold text-white">{t(lang, feature.title)}</h3>
                <p className="mt-1 text-xs leading-relaxed text-surface-500">{t(lang, feature.desc)}</p>
              </motion.div>
            );
          })}
        </div>

        {/* Visual architecture diagram */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="mx-auto mt-16 max-w-4xl"
        >
          <div className="overflow-hidden rounded-2xl border border-white/5 bg-surface-900/50 p-8 md:p-10">
            <div className="text-center text-xs font-semibold uppercase tracking-wider text-surface-500 mb-8">
              {t(lang, 'san_diagram_title')}
            </div>

            {/* Node layer with storage */}
            <div className="grid grid-cols-3 gap-4 md:gap-6">
              {[
                { name: 'Node 1', disks: 3 },
                { name: 'Node 2', disks: 4 },
                { name: 'Node 3', disks: 2 },
              ].map((node) => (
                <div key={node.name} className="text-center">
                  <div className="rounded-xl border border-white/10 bg-white/[0.03] px-3 py-3 md:px-5 md:py-4">
                    <div className="text-xs font-semibold text-white">{node.name}</div>
                    <div className="mt-2 flex justify-center gap-1.5">
                      {Array.from({ length: node.disks }).map((_, j) => (
                        <div
                          key={j}
                          className="flex h-7 w-7 items-center justify-center rounded-md border border-primary-500/20 bg-primary-500/10"
                        >
                          <HardDrive size={12} className="text-primary-400/70" />
                        </div>
                      ))}
                    </div>
                    <div className="mt-1.5 text-[10px] text-surface-500">
                      {node.disks} {t(lang, 'san_disks')}
                    </div>
                  </div>
                </div>
              ))}
            </div>

            {/* Replication arrows */}
            <div className="my-4 flex items-center justify-center gap-2">
              <div className="h-px flex-1 bg-gradient-to-r from-transparent via-primary-500/30 to-transparent" />
              <span className="rounded-full border border-primary-500/20 bg-primary-500/10 px-3 py-1 text-[10px] font-medium text-primary-400">
                {t(lang, 'san_auto_replication')}
              </span>
              <div className="h-px flex-1 bg-gradient-to-r from-transparent via-primary-500/30 to-transparent" />
            </div>

            {/* Unified volume layer */}
            <div className="rounded-xl border border-primary-500/20 bg-primary-500/5 px-6 py-4">
              <div className="text-center text-xs font-bold text-primary-400">{t(lang, 'san_unified_pool')}</div>
              <div className="mt-2 grid grid-cols-3 gap-3 text-center">
                <div className="rounded-lg border border-white/5 bg-white/[0.03] px-2 py-2">
                  <div className="text-[10px] font-semibold text-white">Volume A</div>
                  <div className="text-[10px] text-surface-500">FTT 1 &middot; Stripe</div>
                </div>
                <div className="rounded-lg border border-white/5 bg-white/[0.03] px-2 py-2">
                  <div className="text-[10px] font-semibold text-white">Volume B</div>
                  <div className="text-[10px] text-surface-500">FTT 2 &middot; Mirror</div>
                </div>
                <div className="rounded-lg border border-white/5 bg-white/[0.03] px-2 py-2">
                  <div className="text-[10px] font-semibold text-white">Volume C</div>
                  <div className="text-[10px] text-surface-500">FTT 1 &middot; RAID-10</div>
                </div>
              </div>
            </div>

            {/* FUSE mount hint */}
            <div className="mt-4 flex items-center justify-center gap-3">
              <div className="h-px w-12 bg-primary-500/20" />
              <div className="rounded-lg border border-dashed border-primary-500/20 bg-primary-500/5 px-4 py-2 text-center">
                <span className="text-[10px] font-medium uppercase tracking-wider text-primary-400/60">
                  FUSE Mount
                </span>
                <div className="text-[10px] text-surface-500 mt-0.5">/vmm/san/volume-a/ &middot; /vmm/san/volume-b/ &middot; ...</div>
              </div>
              <div className="h-px w-12 bg-primary-500/20" />
            </div>
          </div>
        </motion.div>

        {/* FTT comparison */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="mx-auto mt-8 max-w-4xl"
        >
          <div className="overflow-hidden rounded-2xl border border-white/5 bg-white/[0.02]">
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-white/5">
                    <th className="px-6 py-4 text-xs font-semibold uppercase tracking-wider text-surface-400">
                      {t(lang, 'san_ftt_policy')}
                    </th>
                    <th className="px-6 py-4 text-xs font-semibold uppercase tracking-wider text-surface-400">
                      {t(lang, 'san_ftt_copies')}
                    </th>
                    <th className="px-6 py-4 text-xs font-semibold uppercase tracking-wider text-surface-400">
                      {t(lang, 'san_ftt_tolerance')}
                    </th>
                    <th className="px-6 py-4 text-xs font-semibold uppercase tracking-wider text-surface-400">
                      {t(lang, 'san_ftt_capacity')}
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-white/5">
                  {[
                    ['FTT 0', '1×', t(lang, 'san_ftt_none'), '100%'],
                    ['FTT 1', '2×', t(lang, 'san_ftt_one_node'), '50%'],
                    ['FTT 2', '3×', t(lang, 'san_ftt_two_nodes'), '33%'],
                  ].map(([policy, copies, tolerance, capacity]) => (
                    <tr key={policy} className="transition-colors hover:bg-white/[0.02]">
                      <td className="whitespace-nowrap px-6 py-3 font-medium text-white">{policy}</td>
                      <td className="px-6 py-3 font-mono text-xs text-surface-400">{copies}</td>
                      <td className="px-6 py-3 text-xs text-surface-400">{tolerance}</td>
                      <td className="px-6 py-3 font-mono text-xs text-surface-400">{capacity}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
