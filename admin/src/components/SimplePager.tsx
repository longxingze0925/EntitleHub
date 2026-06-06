import { Button, Space, Typography } from "antd";

interface SimplePagerProps {
  page: number;
  pageSize: number;
  itemCount: number;
  loading?: boolean;
  onChange: (page: number) => void;
}

export function SimplePager({
  page,
  pageSize,
  itemCount,
  loading,
  onChange
}: SimplePagerProps) {
  const canGoNext = itemCount >= pageSize;

  return (
    <div className="simple-pager">
      <Typography.Text type="secondary">
        第 {page} 页 · 本页 {itemCount} 条
      </Typography.Text>
      <Space>
        <Button
          size="small"
          disabled={page <= 1 || loading}
          onClick={() => onChange(page - 1)}
        >
          上一页
        </Button>
        <Button
          size="small"
          disabled={!canGoNext || loading}
          onClick={() => onChange(page + 1)}
        >
          下一页
        </Button>
      </Space>
    </div>
  );
}
