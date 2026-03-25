import { motion } from 'framer-motion';
import { BarChart3, ShieldCheck, ArrowRightLeft, Globe, Users, Bell, HardDrive, Wrench } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface ClusterProps {
  lang: Lang;
}

const features = [
  { icon: BarChart3, title: 'cluster_drs', desc: 'cluster_drs_desc', highlight: true },
  { icon: ShieldCheck, title: 'cluster_ha', desc: 'cluster_ha_desc', highlight: true },
  { icon: ArrowRightLeft, title: 'cluster_migration', desc: 'cluster_migration_desc', highlight: true },
  { icon: Globe, title: 'cluster_sdn', desc: 'cluster_sdn_desc', highlight: true },
  { icon: HardDrive, title: 'cluster_storage', desc: 'cluster_storage_desc', highlight: false },
  { icon: Users, title: 'cluster_ldap', desc: 'cluster_ldap_desc', highlight: false },
  { icon: Wrench, title: 'cluster_maintenance', desc: 'cluster_maintenance_desc', highlight: false },
  { icon: Bell, title: 'cluster_notifications', desc: 'cluster_notifications_desc', highlight: false },
] as const;

export default function Cluster({ lang }: ClusterProps) {
  return (
    <section id="cluster" className="relative py-24 md:py-32">
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-transparent via-accent-400/[0.02] to-transparent" />

      <div className="relative mx-auto max-w-7xl px-6">
        {/* Header */}
        <div className="mx-auto max-w-2xl text-center">
          <motion.span
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="inline-block rounded-full border border-accent-400/20 bg-accent-400/10 px-3 py-1 text-xs font-semibold uppercase tracking-wider text-accent-400"
          >
            {t(lang, 'cluster_badge')}
          </motion.span>
          <motion.h2
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="mt-4 text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
          >
            {t(lang, 'cluster_title')}
          </motion.h2>
          <motion.p
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="mt-4 text-lg text-surface-400"
          >
            {t(lang, 'cluster_subtitle')}
          </motion.p>
        </div>

        {/* Top 4 — big cards (DRS, HA, Migration, SDN) */}
        <div className="mt-16 grid gap-6 sm:grid-cols-2">
          {features.slice(0, 4).map((feature, i) => {
            const Icon = feature.icon;
            return (
              <motion.div
                key={feature.title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: i * 0.08 }}
                className="group relative overflow-hidden rounded-2xl border border-white/5 bg-white/[0.02] p-8 transition-all hover:border-accent-400/20 hover:bg-white/[0.04]"
              >
                <div className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-accent-400/40 to-transparent" />
                <div className="pointer-events-none absolute inset-0 rounded-2xl opacity-0 transition-opacity group-hover:opacity-100 bg-gradient-to-br from-accent-400/5 to-transparent" />

                <div className="relative">
                  <div className="mb-5 inline-flex rounded-xl border border-accent-400/20 bg-accent-400/10 p-3">
                    <Icon size={22} className="text-accent-400" />
                  </div>
                  <h3 className="text-lg font-bold text-white">{t(lang, feature.title)}</h3>
                  <p className="mt-2 text-sm leading-relaxed text-surface-400">{t(lang, feature.desc)}</p>
                </div>
              </motion.div>
            );
          })}
        </div>

        {/* Bottom 4 — compact list */}
        <div className="mt-8 grid gap-6 sm:grid-cols-2 lg:grid-cols-4">
          {features.slice(4).map((feature, i) => {
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

        {/* Visual cluster diagram */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="mx-auto mt-16 max-w-4xl"
        >
          <div className="overflow-hidden rounded-2xl border border-white/5 bg-surface-900/50 p-8 md:p-10">
            <div className="text-center text-xs font-semibold uppercase tracking-wider text-surface-500 mb-8">Cluster Architecture</div>

            {/* Central Authority */}
            <div className="mx-auto mb-8 w-fit rounded-xl border border-accent-400/30 bg-accent-400/10 px-8 py-4">
              <div className="text-sm font-bold text-accent-400">Cluster Authority</div>
              <div className="mt-1 flex gap-3 text-xs text-surface-400">
                <span className="rounded bg-accent-400/10 px-1.5 py-0.5">DRS</span>
                <span className="rounded bg-accent-400/10 px-1.5 py-0.5">HA</span>
                <span className="rounded bg-accent-400/10 px-1.5 py-0.5">SDN</span>
                <span className="rounded bg-accent-400/10 px-1.5 py-0.5">Migration</span>
              </div>
            </div>

            {/* Connection lines */}
            <div className="flex justify-center mb-2">
              <div className="grid grid-cols-3 gap-6 md:gap-10">
                {[
                  { name: 'Node 1', vms: 4, cpu: '32%', mem: '48%' },
                  { name: 'Node 2', vms: 6, cpu: '67%', mem: '72%' },
                  { name: 'Node 3', vms: 2, cpu: '12%', mem: '24%' },
                ].map((node) => (
                  <div key={node.name} className="text-center">
                    <div className="mx-auto mb-3 h-8 w-px bg-gradient-to-b from-accent-400/50 to-primary-500/30" />
                    <div className="rounded-xl border border-white/10 bg-white/[0.03] px-4 py-3 md:px-6 md:py-4">
                      <div className="text-xs font-semibold text-white">{node.name}</div>
                      <div className="mt-2 space-y-1 text-[10px] text-surface-500">
                        <div>{node.vms} VMs &middot; KVM</div>
                        <div>CPU {node.cpu} &middot; RAM {node.mem}</div>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </div>

            {/* SDN overlay hint */}
            <div className="mt-6 mx-auto max-w-xs rounded-lg border border-dashed border-accent-400/20 bg-accent-400/5 px-4 py-2 text-center">
              <span className="text-[10px] font-medium uppercase tracking-wider text-accent-400/60">SDN Overlay Network</span>
              <div className="text-[10px] text-surface-500 mt-0.5">DHCP &middot; DNS &middot; PXE &middot; Network Isolation</div>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
