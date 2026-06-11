import { Alert, Button, Input, Select, Space, Table, Tag, Typography } from "antd";
import type { ColumnsType } from "antd/es/table";
import { RefreshCw, Search, X } from "lucide-react";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  listAiWalletLedgerEntries,
  type AiWalletLedgerEntry
} from "../../api/admin";
import { SimplePager } from "../../components/SimplePager";
import { dateTime, shortId } from "../../utils/format";
import { tMessage } from "../../utils/i18n";

const pageSize = 20;

const ledgerTypeOptions = [
  { label: "全部类型", value: "" },
  { label: "充值", value: "credit" },
  { label: "扣减", value: "debit" },
  { label: "预扣", value: "hold" },
  { label: "结算", value: "capture" },
  { label: "释放", value: "release" },
  { label: "退款", value: "refund" },
  { label: "调整", value: "adjustment" }
];

export function AiWalletLedgerPage() {
  const [draftCustomerId, setDraftCustomerId] = useState("");
  const [draftReferenceId, setDraftReferenceId] = useState("");
  const [draftEntryType, setDraftEntryType] = useState("");
  const [customerId, setCustomerId] = useState("");
  const [referenceId, setReferenceId] = useState("");
  const [entryType, setEntryType] = useState("");
  const [page, setPage] = useState(1);

  const query = useQuery({
    queryKey: ["admin", "ai-wallet-ledger", customerId, entryType, referenceId, page],
    queryFn: () =>
      listAiWalletLedgerEntries({
        customer_id: customerId,
        entry_type: entryType,
        reference_id: referenceId,
        page,
        page_size: pageSize
      })
  });

  const applyFilters = () => {
    setPage(1);
    setCustomerId(draftCustomerId.trim());
    setReferenceId(draftReferenceId.trim());
    setEntryType(draftEntryType);
  };

  const resetFilters = () => {
    setPage(1);
    setDraftCustomerId("");
    setDraftReferenceId("");
    setDraftEntryType("");
    setCustomerId("");
    setReferenceId("");
    setEntryType("");
  };

  const columns: ColumnsType<AiWalletLedgerEntry> = [
    {
      title: "客户",
      dataIndex: "customer_email",
      key: "customer_email",
      width: 320,
      render: (value: string | null | undefined, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={record.customer_name || value || "-"}>
            {record.customer_name || value || "-"}
          </Typography.Text>
          <Typography.Text ellipsis title={value || record.customer_id} type="secondary">
            {value || shortId(record.customer_id)}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "类型",
      dataIndex: "entry_type",
      key: "entry_type",
      width: 100,
      render: (value: string) => <Tag>{ledgerTypeLabel(value)}</Tag>
    },
    {
      title: "金额",
      dataIndex: "amount_minor",
      key: "amount_minor",
      width: 130,
      render: (value: number, record) => (
        <Typography.Text type={value < 0 ? "danger" : "success"}>
          {money(value, record.currency)}
        </Typography.Text>
      )
    },
    {
      title: "余额",
      dataIndex: "balance_after_minor",
      key: "balance_after_minor",
      width: 130,
      render: (value: number, record) => money(value, record.currency)
    },
    {
      title: "冻结",
      dataIndex: "held_after_minor",
      key: "held_after_minor",
      width: 130,
      render: (value: number, record) => money(value, record.currency)
    },
    {
      title: "原因",
      dataIndex: "reason",
      key: "reason",
      width: 260,
      render: (value: string) => (
        <Typography.Text ellipsis title={value}>
          {value}
        </Typography.Text>
      )
    },
    {
      title: "引用",
      key: "reference",
      width: 220,
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>{record.reference_type ?? "-"}</Typography.Text>
          <Typography.Text type="secondary">{shortId(record.reference_id)}</Typography.Text>
        </Space>
      )
    },
    {
      title: "时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      render: (value: string) => dateTime(value)
    }
  ];

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>计费流水</Typography.Title>
          <Typography.Text type="secondary">AI 余额充值、预扣、结算、退款和后台调整记录</Typography.Text>
        </div>
        <Space className="page-heading-actions">
          <Input
            allowClear
            placeholder="客户 ID"
            value={draftCustomerId}
            onChange={(event) => setDraftCustomerId(event.target.value)}
            onPressEnter={applyFilters}
            className="audit-filter-input"
          />
          <Select
            options={ledgerTypeOptions}
            value={draftEntryType}
            onChange={setDraftEntryType}
            className="audit-filter-input"
          />
          <Input
            allowClear
            placeholder="引用 ID"
            value={draftReferenceId}
            onChange={(event) => setDraftReferenceId(event.target.value)}
            onPressEnter={applyFilters}
            className="audit-filter-input"
          />
          <Button type="primary" icon={<Search size={16} />} onClick={applyFilters}>
            查询
          </Button>
          <Button icon={<X size={16} />} onClick={resetFilters}>
            清空
          </Button>
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
        </Space>
      </div>

      {query.error ? <Alert type="error" message={tMessage("ai_wallet_ledger_load_failed")} /> : null}

      <Table
        rowKey="id"
        loading={query.isLoading}
        columns={columns}
        dataSource={query.data?.items ?? []}
        pagination={false}
        scroll={{ x: "max-content" }}
        locale={{ emptyText: "暂无数据" }}
      />
      <SimplePager
        page={page}
        pageSize={pageSize}
        itemCount={query.data?.items.length ?? 0}
        loading={query.isFetching}
        onChange={setPage}
      />
    </section>
  );
}

function ledgerTypeLabel(value: string): string {
  const labels: Record<string, string> = {
    credit: "充值",
    debit: "扣减",
    hold: "预扣",
    capture: "结算",
    release: "释放",
    refund: "退款",
    adjustment: "调整"
  };

  return labels[value] ?? value;
}

function money(value: number, currency: string): string {
  const sign = value < 0 ? "-" : "";
  const amount = Math.abs(value) / 100;

  return `${sign}${currency} ${amount.toFixed(2)}`;
}
