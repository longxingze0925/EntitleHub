import { Space, Switch, Typography } from "antd";

export function HistoryToggle(props: {
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <Space size={6} className="history-toggle">
      <Switch size="small" checked={props.checked} onChange={props.onChange} />
      <Typography.Text type="secondary">显示历史</Typography.Text>
    </Space>
  );
}
