// SPDX-License-Identifier: GPL-2.0
//
// ClaudeBox XDP network filter — IPv4 allowlist enforcement
//
// Governs VM egress only. Claude Code's host-side Anthropic API calls
// are NOT subject to this filter. Do NOT add api.anthropic.com here.
//
// Compile (inside Linux VM):
//   clang -O2 -target bpf -c filter.c -o filter.o
//   llvm-strip -g filter.o

#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/udp.h>
#include <linux/in.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

// ---------------------------------------------------------------------------
// Map: allowed IPv4 destination addresses
// Key: __u32 (network byte order)  Value: __u8 (unused, set to 1)
// Max 256 entries — covers typical multi-language allowlists.
// ---------------------------------------------------------------------------
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 256);
    __type(key, __u32);
    __type(value, __u8);
} allowed_ips SEC(".maps");

// ---------------------------------------------------------------------------
// XDP program entry point
// ---------------------------------------------------------------------------
SEC("xdp")
int network_filter(struct xdp_md *ctx)
{
    void *data_end = (void *)(long)ctx->data_end;
    void *data     = (void *)(long)ctx->data;

    // ------------------------------------------------------------------
    // Step 1: Parse ethernet header — pass non-IP traffic unchanged.
    // We never want to block ARP, IPv6 NDP, etc.
    // ------------------------------------------------------------------
    struct ethhdr *eth = data;
    if ((void *)(eth + 1) > data_end)
        return XDP_PASS;

    if (bpf_ntohs(eth->h_proto) != ETH_P_IP)
        return XDP_PASS;

    // ------------------------------------------------------------------
    // Step 2: Parse IPv4 header and extract destination IP.
    // ------------------------------------------------------------------
    struct iphdr *ip = (void *)(eth + 1);
    if ((void *)(ip + 1) > data_end)
        return XDP_PASS;

    __u32 dest_ip = ip->daddr;  // network byte order

    // ------------------------------------------------------------------
    // Step 3: Always allow loopback (127.0.0.1 == 0x0100007f in NBO).
    // ------------------------------------------------------------------
    if (dest_ip == bpf_htonl(0x7f000001u))
        return XDP_PASS;

    // ------------------------------------------------------------------
    // Step 4: Always allow UDP port 53 (DNS).
    // This permits the VM to resolve names regardless of the allowlist.
    // ------------------------------------------------------------------
    if (ip->protocol == IPPROTO_UDP) {
        struct udphdr *udp = (void *)((void *)ip + (ip->ihl * 4));
        if ((void *)(udp + 1) > data_end)
            return XDP_PASS;  // malformed — pass safely
        if (bpf_ntohs(udp->dest) == 53)
            return XDP_PASS;
    }

    // ------------------------------------------------------------------
    // Step 5: Lookup destination IP in the allowlist map.
    // ------------------------------------------------------------------
    __u8 *allowed = bpf_map_lookup_elem(&allowed_ips, &dest_ip);

    // ------------------------------------------------------------------
    // Step 6: Pass if found, drop otherwise.
    // ------------------------------------------------------------------
    return (allowed != NULL) ? XDP_PASS : XDP_DROP;
}

char _license[] SEC("license") = "GPL";
