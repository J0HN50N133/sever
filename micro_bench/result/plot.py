import marimo

__generated_with = "0.18.0"
app = marimo.App()


@app.cell
def ___(mo):
    mo.md(r"""
    # Plotting Dashboard
    This notebook contains visualizations for benchmark results.
    """)
    return


@app.cell
def common():
    import json
    import matplotlib.pyplot as plt
    import numpy as np
    import seaborn as sns
    from matplotlib.ticker import ScalarFormatter

    # --- File Paths ---
    BENCHMARK_RESULTS_FILE = 'benchmark_results.json'
    BATCH_BENCHMARK_RESULTS_FILE = 'batch_benchmark_results.json'
    VERIFICATION_RESULTS_FILE = 'verification_results.json'

    # --- Plotting Styles ---
    PLOT_STYLE = {
        "figsize": (10.5, 6.5),
        "legend_fontsize": 24,
        "label_fontsize": 26,
        "tick_fontsize": 24,
        "linewidth": 3,
        "markersize": 12,
        "marker_edgewidth": 2,
        "grid_alpha": 0.3,
    }

    # --- Figure Saving Options ---
    SAVE_PNG = False
    SAVE_PDF = True
    SAVE_EPS = True
    return (
        BATCH_BENCHMARK_RESULTS_FILE,
        BENCHMARK_RESULTS_FILE,
        PLOT_STYLE,
        ScalarFormatter,
        VERIFICATION_RESULTS_FILE,
        json,
        np,
        plt,
        sns,
    )


@app.cell
def utils_func(plt, sns):
    def setup_plot_style(style_config):
        """Sets up the global plotting style for seaborn and matplotlib."""
        sns.set_theme(style="whitegrid")
        sns.set_context("paper")

        plt.rcParams["figure.figsize"] = style_config["figsize"]
        plt.rcParams["font.size"] = style_config["tick_fontsize"]
        plt.rcParams["font.family"] = "DejaVu Sans"
        plt.rcParams["axes.grid"] = True
        plt.rcParams["grid.alpha"] = style_config["grid_alpha"]
        plt.rcParams["axes.spines.right"] = False
        plt.rcParams["axes.spines.top"] = False

        return sns.color_palette("deep", n_colors=10)

    def set_ax_border(ax):
        """Set a visible border on all sides of the axes."""
        for spine in ax.spines.values():
            spine.set_visible(True)
            spine.set_linewidth(1)

    def save_fig(plt_instance, base_path, png=False, pdf=True, eps=True):
        """Saves the current figure to specified formats."""
        if png:
            plt_instance.savefig(f'{base_path}.png', dpi=300, bbox_inches='tight', facecolor='white')
        if pdf:
            plt_instance.savefig(f'{base_path}.pdf', bbox_inches='tight', facecolor='white')
        if eps:
            plt_instance.savefig(f'{base_path}.eps', bbox_inches='tight', facecolor='white', transparent=True)
    return save_fig, set_ax_border, setup_plot_style


@app.cell
def ___(mo):
    mo.md(r"""
    # Figure 1: Issuer Overheads
    """)
    return


@app.cell
def issuer_compute_overheads(
    BENCHMARK_RESULTS_FILE,
    PLOT_STYLE,
    ScalarFormatter,
    json,
    np,
    plt,
    save_fig,
    set_ax_border,
    setup_plot_style,
):
    # --- Style and Data Loading ---
    _colors = setup_plot_style(PLOT_STYLE)
    with open(BENCHMARK_RESULTS_FILE, 'r') as _f:
        _sever_data = json.load(_f)
    with open('prevoke_issuer_overheads_results.json', 'r') as _f:
        _prevoke_data = json.load(_f)

    # --- Data Extraction and Processing ---
    def _extract_issuer_data(raw_data, prefix=""):
        """Extracts and structures issuer overhead data for plotting."""
        _metrics = raw_data['metrics']
        _labels, _counts = set(), set()
        _all_durations = []

        for _key, _value in _metrics.items():
            if '_' in _key:
                _parts = _key.split('_')
                _label, _count_str = '_'.join(_parts[:-1]), _parts[-1]
                _labels.add(prefix + _label)
                _counts.add(int(_count_str))
                _all_durations.append(_value['duration_ms'] / 1000.0)

        _sorted_labels = sorted(list(_labels))
        _sorted_counts = sorted(list(_counts))

        _plot_data = {}
        for _label in _sorted_labels:
            _y_values = []
            for _count in _sorted_counts:
                # Handle both prefixed and non-prefixed label matching
                clean_label = _label.replace(prefix, "")
                _metric = _metrics.get(f'{clean_label}_{_count}')
                _y_values.append(_metric['duration_ms'] / 1000.0 if _metric else None)
            _plot_data[_label] = _y_values

        return _sorted_labels, _sorted_counts, _plot_data, _all_durations

    def _format_x_labels(counts):
        """Formats large numbers into k (thousands) or M (millions)."""
        _labels = []
        for _count in counts:
            if _count >= 1_000_000:
                _labels.append(f'{_count // 1_000_000}M')
            elif _count >= 1_000:
                _labels.append(f'{_count // 1_000}k')
            else:
                _labels.append(str(_count))
        return _labels

    # Extract data from both datasets
    _sever_labels, _sever_counts, _sever_plot_data, _sever_durations = _extract_issuer_data(_sever_data, "")
    _prevoke_labels, _prevoke_counts, _prevoke_plot_data, _prevoke_durations = _extract_issuer_data(_prevoke_data, "Prevoke-")

    # Combine data from both datasets
    _all_labels = list(_sever_labels) + list(_prevoke_labels)
    _all_counts = sorted(set(_sever_counts + _prevoke_counts))
    _all_durations = _sever_durations + _prevoke_durations
    _combined_plot_data = {**_sever_plot_data, **_prevoke_plot_data}

    _labels = sorted(_all_labels)
    _counts = _all_counts
    _plot_data = _combined_plot_data
    _x_labels = _format_x_labels(_counts)
    _max_duration = max(_all_durations) if _all_durations else 0
    _y_max = int(np.ceil(_max_duration / 10)) * 10

    # --- Plotting ---
    _fig, _ax = plt.subplots(figsize=PLOT_STYLE["figsize"])
    _markers = ['o', 's', '^', 'D', 'v', '<', '>', 'p', '*', 'h']

    for i, _label in enumerate(_labels):
        _y_values = _plot_data[_label]
        _valid_points = [(np.log10(c), y) for c, y in zip(_counts, _y_values) if y is not None]
        if _valid_points:
            _x_valid, _y_valid = zip(*_valid_points)
            # Use dashed line style for Prevoke data, solid for others
            _linestyle = '--' if _label.startswith('Prevoke-') else '-'
            _ax.plot(_x_valid, _y_valid, color=_colors[i], marker=_markers[i % len(_markers)],
                    linewidth=PLOT_STYLE["linewidth"], markersize=PLOT_STYLE["markersize"],
                    label=_label, markerfacecolor='white', markeredgewidth=PLOT_STYLE["marker_edgewidth"],
                    linestyle=_linestyle)

    # --- Axes and Legend Configuration ---
    _ax.set_xscale('log')
    _ax.set_xticks(np.log10(_counts))
    _ax.set_xticklabels(_x_labels, fontsize=PLOT_STYLE["tick_fontsize"])
    _ax.xaxis.set_minor_formatter(plt.NullFormatter())
    _ax.tick_params(axis='x', which='minor', bottom=False)

    _ax.set_xlabel('Total number of clients', fontsize=PLOT_STYLE["label_fontsize"])
    _ax.set_ylabel('Overheads (seconds)', fontsize=PLOT_STYLE["label_fontsize"])

    _ax.set_ylim(0, _y_max + 5)
    _ax.set_yticks(np.arange(0, _y_max + 1, 10))
    _ax.set_yticklabels([str(int(t)) for t in np.arange(0, _y_max + 1, 10)], fontsize=10)
                        #PLOT_STYLE["tick_fontsize"])
    _ax.yaxis.set_major_formatter(ScalarFormatter())

    set_ax_border(_ax)
    _ax.grid(True, linestyle='--', alpha=PLOT_STYLE["grid_alpha"])

    _legend = _ax.legend(loc='best', fontsize=PLOT_STYLE["legend_fontsize"], frameon=True, ncol=1, fancybox=True, shadow=True)
    _legend.get_frame().set_facecolor('white')
    _legend.get_frame().set_alpha(0.9)

    plt.tight_layout()
    save_fig(plt, 'issuer_overheads')
    plt.show()
    return


@app.cell
def ___(mo):
    mo.md(r"""
    # Figure 2: E2E Throughput vs. Batch Size
    """)
    return


@app.cell
def _(
    BATCH_BENCHMARK_RESULTS_FILE,
    PLOT_STYLE,
    json,
    plt,
    save_fig,
    set_ax_border,
    setup_plot_style,
):
    # --- Style and Data Loading ---
    _colors = setup_plot_style(PLOT_STYLE)
    with open(BATCH_BENCHMARK_RESULTS_FILE, 'r') as _f:
        _data = json.load(_f)

    # --- Data Extraction ---
    def _extract_throughput_data(raw_data, scenario_prefix, categories):
        """Generic function to extract throughput data based on scenario prefixes."""
        _extracted_data = {_cat: {} for _cat in categories} if categories else {"all": {}}
        for _key, _metric in raw_data['metrics'].items():
            if _key.startswith(scenario_prefix) and 'throughput_ops_per_sec' in _metric and _metric['throughput_ops_per_sec'] is not None:
                _parts = _key.split('_')
                _batch_size = int(_parts[1])

                _found_category = False
                if categories:
                    for _cat in categories:
                        if _cat in _key:
                            _extracted_data[_cat][_batch_size] = _metric['throughput_ops_per_sec']
                            _found_category = True
                            break
                if not categories and not _found_category:
                     _extracted_data["all"][_batch_size] = _metric['throughput_ops_per_sec']

        return _extracted_data

    _s1_data = _extract_throughput_data(_data, 'S1-Issuance', [])["all"]
    _s2_data = _extract_throughput_data(_data, 'S2-Revoke', ['10%', '25%', '50%'])
    _s3_data = _extract_throughput_data(_data, 'S3-Concurrent', ['10%', '25%', '50%'])

    # --- Plotting Configuration ---
    _PLOT_CONFIG = {
        'Issuance (S1)': {'data': _s1_data, 'color': _colors[0], 'marker': 'o', 'linestyle': '-'},
        'Revocation 10% (S2)': {'data': _s2_data.get('10%', {}), 'color': _colors[1], 'marker': 's', 'linestyle': '-'},
        'Revocation 25% (S2)': {'data': _s2_data.get('25%', {}), 'color': _colors[2], 'marker': '^', 'linestyle': '-'},
        'Revocation 50% (S2)': {'data': _s2_data.get('50%', {}), 'color': _colors[3], 'marker': 'D', 'linestyle': '-'},
        'Concurrent Verification 10% (S3)': {'data': _s3_data.get('10%', {}), 'color': _colors[4], 'marker': 'v', 'linestyle': '--'},
        'Concurrent Verification 25% (S3)': {'data': _s3_data.get('25%', {}), 'color': _colors[5], 'marker': '<', 'linestyle': '-.'},
        'Concurrent Verification 50% (S3)': {'data': _s3_data.get('50%', {}), 'color': _colors[6], 'marker': '>', 'linestyle': ':'},
    }

    # --- Plotting ---
    _fig, _ax = plt.subplots(figsize=PLOT_STYLE["figsize"])
    _all_batch_sizes = set()

    for _label, _config in _PLOT_CONFIG.items():
        if _config['data']:
            _batch_sizes = sorted(_config['data'].keys())
            _throughputs = [_config['data'][bs] for bs in _batch_sizes]
            _all_batch_sizes.update(_batch_sizes)
            _ax.plot(_batch_sizes, _throughputs, label=_label, color=_config['color'], marker=_config['marker'],
                    linestyle=_config['linestyle'], linewidth=PLOT_STYLE["linewidth"], markersize=PLOT_STYLE["markersize"],
                    markerfacecolor='white', markeredgewidth=PLOT_STYLE["marker_edgewidth"])

    # --- Axes and Legend ---
    set_ax_border(_ax)
    _ax.set_xscale('log')
    _ax.set_yscale('log')
    _ax.set_xlabel('Batch size', fontsize=PLOT_STYLE["label_fontsize"])
    _ax.set_ylabel('Throughput (tps)', fontsize=PLOT_STYLE["label_fontsize"])
    _ax.grid(True, linestyle='--', alpha=PLOT_STYLE["grid_alpha"])

    _sorted_batch_sizes = sorted(list(_all_batch_sizes))
    _ax.set_xticks(_sorted_batch_sizes)
    _ax.set_xticklabels([str(bs) for bs in _sorted_batch_sizes], fontsize=PLOT_STYLE["tick_fontsize"])
    _ax.tick_params(axis='y', labelsize=PLOT_STYLE["tick_fontsize"])

    _ax.legend(loc='best', fontsize=PLOT_STYLE["legend_fontsize"], frameon=True, fancybox=True, shadow=True, ncol=1)

    plt.tight_layout()
    save_fig(plt, 'e2e_batch_size_throughput')
    plt.show()
    return


@app.cell
def ___(mo):
    mo.md(r"""
    # Figure 3: E2E Latency vs. Batch Size
    """)
    return


@app.cell
def _(
    BATCH_BENCHMARK_RESULTS_FILE,
    PLOT_STYLE,
    VERIFICATION_RESULTS_FILE,
    json,
    plt,
    save_fig,
    set_ax_border,
    setup_plot_style,
):
    # --- Style and Data Loading ---
    _colors = setup_plot_style(PLOT_STYLE)
    with open(VERIFICATION_RESULTS_FILE, 'r') as _f:
        _verification_data = json.load(_f)
    with open(BATCH_BENCHMARK_RESULTS_FILE, 'r') as _f:
        _batch_data = json.load(_f)

    # --- Data Extraction ---
    def _extract_latency_data(raw_data):
        """Extracts average latency data, grouping by scenario."""
        _latency_data = {}
        for _key, _metric in raw_data['metrics'].items():
            if 'avg_latency_ms' in _metric and _metric['avg_latency_ms'] is not None:
                _parts = _key.split('_')
                _batch_size = int(_parts[-1])
                _scenario = '_'.join(_parts[:-1])
                if _scenario not in _latency_data:
                    _latency_data[_scenario] = {}
                _latency_data[_scenario][_batch_size] = _metric['avg_latency_ms']
        return _latency_data

    _verification_latency = _extract_latency_data(_verification_data)
    _batch_latency = _extract_latency_data(_batch_data)

    # --- Plotting Configuration ---
    _PLOT_CONFIG = {
        'Auth.10% Revoke(S3)': {'data': _verification_latency.get('S3-Concurrent-10%-Verification', {}), 'color': _colors[1], 'marker': 'o', 'linestyle': '--'},
        'Auth.25% Revoke(S3)': {'data': _verification_latency.get('S3-Concurrent-25%-Verification', {}), 'color': _colors[2], 'marker': 's', 'linestyle': '--'},
        'Auth.50% Revoke(S3)': {'data': _verification_latency.get('S3-Concurrent-50%-Verification', {}), 'color': _colors[3], 'marker': '^', 'linestyle': '--'},
        'Issuance (S1)': {'data': _batch_latency.get('S1-Issuance-Batch', {}), 'color': _colors[0], 'marker': 'D', 'linestyle': '-'},
        'Revocation 10% (S2)': {'data': _batch_latency.get('S2-Revoke10%-Batch', {}), 'color': _colors[4], 'marker': 'v', 'linestyle': '-'},
        'Revocation 25% (S2)': {'data': _batch_latency.get('S2-Revoke25%-Batch', {}), 'color': _colors[5], 'marker': '<', 'linestyle': '-'},
        'Revocation 50% (S2)': {'data': _batch_latency.get('S2-Revoke50%-Batch', {}), 'color': _colors[6], 'marker': '>', 'linestyle': '-'},
    }

    # --- Plotting ---
    _fig, _ax = plt.subplots(figsize=PLOT_STYLE["figsize"])
    _all_batch_sizes = set()

    for _label, _config in _PLOT_CONFIG.items():
        if _config['data']:
            _batch_sizes = sorted(_config['data'].keys())
            _latencies = [_config['data'][bs] for bs in _batch_sizes]
            _all_batch_sizes.update(_batch_sizes)
            _ax.plot(_batch_sizes, _latencies, label=_label, color=_config['color'], marker=_config['marker'],
                    linestyle=_config['linestyle'], linewidth=PLOT_STYLE["linewidth"], markersize=PLOT_STYLE["markersize"],
                    markerfacecolor='white', markeredgewidth=PLOT_STYLE["marker_edgewidth"])

    # --- Axes and Legend ---
    set_ax_border(_ax)
    _ax.set_xscale('log')
    _sorted_batch_sizes = sorted(list(_all_batch_sizes))
    _ax.set_xticks(_sorted_batch_sizes)
    _ax.set_xticklabels([str(bs) for bs in _sorted_batch_sizes], fontsize=PLOT_STYLE["tick_fontsize"])

    _ax.set_xlabel('Batch size', fontsize=PLOT_STYLE["label_fontsize"])
    _ax.set_ylabel('Average latency (ms)', fontsize=PLOT_STYLE["label_fontsize"])
    _ax.grid(True, linestyle='--', alpha=PLOT_STYLE["grid_alpha"])
    _ax.tick_params(axis='y', labelsize=PLOT_STYLE["tick_fontsize"])

    _legend = plt.legend(loc='lower right', bbox_to_anchor=(0.97, 0.12), fontsize=18, frameon=True, fancybox=True, shadow=True, ncol=1)
    _legend.get_frame().set_facecolor('white')
    _legend.get_frame().set_alpha(0.9)

    plt.tight_layout()
    save_fig(plt, 'e2e_batch_size_latency')
    plt.show()
    return


@app.cell
def ___(mo):
    mo.md(r"""
    # Figure 4: Smart Contract Performance
    """)
    return


@app.cell
def _(PLOT_STYLE, np, plt, save_fig, set_ax_border, setup_plot_style):
    # --- Style Setup ---
    _colors = setup_plot_style(PLOT_STYLE)

    # --- Data ---
    _OPERATIONS = ['getAccumulator', 'updateAccumulator']
    _THROUGHPUT_DATA = [528.3, 264.3]  # TPS
    _LATENCY_DATA = [10, 60]  # ms

    # --- Plotting ---
    _fig, _ax1 = plt.subplots(figsize=PLOT_STYLE["figsize"])
    _x_pos = np.arange(len(_OPERATIONS))
    _bar_width = 0.35

    # Throughput bars (left y-axis)
    _ax1.bar(_x_pos - _bar_width / 2, _THROUGHPUT_DATA, _bar_width, label='Throughput (TPS)', color=_colors[0], alpha=0.8)
    _ax1.set_ylabel('Throughput (TPS)', fontsize=PLOT_STYLE["label_fontsize"])
    _ax1.tick_params(axis='y', labelsize=PLOT_STYLE["tick_fontsize"])
    _ax1.set_ylim(0, max(_THROUGHPUT_DATA) * 1.2)

    # Latency bars (right y-axis)
    _ax2 = _ax1.twinx()
    _ax2.bar(_x_pos + _bar_width / 2, _LATENCY_DATA, _bar_width, label='Average Latency (ms)', color=_colors[1], alpha=0.8)
    _ax2.set_ylabel('Average latency (ms)', fontsize=PLOT_STYLE["label_fontsize"])
    _ax2.tick_params(axis='y', labelsize=PLOT_STYLE["tick_fontsize"])
    _ax2.set_ylim(0, max(_LATENCY_DATA) * 1.2)

    # --- Axes and Legend ---
    _ax1.set_xticks(_x_pos)
    _ax1.set_xticklabels(_OPERATIONS, fontsize=PLOT_STYLE["tick_fontsize"])
    _ax1.set_xlabel('Operations', fontsize=PLOT_STYLE["label_fontsize"])

    set_ax_border(_ax1)
    _ax1.grid(False)
    _ax2.grid(False)

    # Combine legends from both axes
    _lines1, _labels1 = _ax1.get_legend_handles_labels()
    _lines2, _labels2 = _ax2.get_legend_handles_labels()
    _ax1.legend(_lines1 + _lines2, _labels1 + _labels2, loc='upper center', fontsize=22, frameon=False)

    plt.tight_layout()
    save_fig(plt, 'smart_contract')
    plt.show()
    return


if __name__ == "__main__":
    app.run()
