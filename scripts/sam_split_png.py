#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
改进的SAM工程图纸视图分割
解决重复分割、不完整分割和遗漏分割问题
"""

import cv2
import numpy as np
import os
from pathlib import Path
import json
import argparse
from sklearn.cluster import DBSCAN

try:
    from segment_anything import sam_model_registry, SamAutomaticMaskGenerator
    import torch
except ImportError:
    print("需要安装 segment-anything 和 torch")
    exit(1)


class ImprovedSAMDrawingSplitter:
    def __init__(self, model_type="vit_b", checkpoint_path=None):
        """
        初始化改进的SAM分割器，增强稳定性
        """
        self.device = "cuda" if torch.cuda.is_available() else "cpu"
        print(f"使用设备: {self.device}")
        
        # 检查内存情况
        if self.device == "cuda":
            torch.cuda.empty_cache()  # 清理GPU内存
            print(f"GPU内存: {torch.cuda.get_device_properties(0).total_memory / 1e9:.1f}GB")
        
        # 加载SAM模型
        if checkpoint_path is None:
            self.download_model(model_type)
            checkpoint_path = f"sam_{model_type}.pth"
        
        try:
            self.sam = sam_model_registry[model_type](checkpoint=checkpoint_path)
            self.sam.to(device=self.device)
            print(f"SAM模型加载成功: {model_type}")
        except Exception as e:
            print(f"SAM模型加载失败: {e}")
            raise
        
        # 创建轻量级的mask生成器，避免内存和计算问题
        self.mask_generators = [
            # 主要生成器 - 平衡效果和性能
            SamAutomaticMaskGenerator(
                model=self.sam,
                points_per_side=16,  # 减少采样点以提高速度
                pred_iou_thresh=0.88,
                stability_score_thresh=0.90,
                crop_n_layers=0,  # 不使用crop层以避免计算复杂度
                crop_n_points_downscale_factor=1,
                min_mask_region_area=3000,
            ),
            # 文本专用生成器 - 轻量级配置
            SamAutomaticMaskGenerator(
                model=self.sam,
                points_per_side=12,  # 更少的采样点
                pred_iou_thresh=0.75,
                stability_score_thresh=0.80,
                crop_n_layers=0,  # 避免crop操作
                crop_n_points_downscale_factor=1,
                min_mask_region_area=1500,
            )
        ]
    
    def download_model(self, model_type):
        """下载SAM模型"""
        model_urls = {
            "vit_b": "https://dl.fbaipublicfiles.com/segment_anything/sam_vit_b_01ec64.pth",
            "vit_l": "https://dl.fbaipublicfiles.com/segment_anything/sam_vit_l_0b3195.pth",
            "vit_h": "https://dl.fbaipublicfiles.com/segment_anything/sam_vit_h_4b8939.pth"
        }
        
        import urllib.request
        filename = f"sam_{model_type}.pth"
        
        if not os.path.exists(filename):
            print(f"正在下载SAM模型: {model_type}")
            urllib.request.urlretrieve(model_urls[model_type], filename)
            print(f"模型下载完成: {filename}")

    def preprocess_image(self, image):
        """
        预处理图像以减少SAM计算负载
        """
        h, w = image.shape[:2]
        
        # 如果图像太大，先进行适当缩放
        max_dimension = 2048  # 最大尺寸限制
        if max(h, w) > max_dimension:
            scale = max_dimension / max(h, w)
            new_h = int(h * scale)
            new_w = int(w * scale)
            
            print(f"图像过大 ({w}x{h})，缩放到 ({new_w}x{new_h})")
            image = cv2.resize(image, (new_w, new_h), interpolation=cv2.INTER_AREA)
            
            # 返回缩放后的图像和缩放比例
            return image, scale
        
        return image, 1.0
    
    def scale_bbox(self, bbox, scale_factor):
        """
        根据缩放比例调整边界框
        """
        if scale_factor == 1.0:
            return bbox
        
        x, y, w, h = bbox
        return [
            int(x / scale_factor),
            int(y / scale_factor),
            int(w / scale_factor),
            int(h / scale_factor)
        ]
    
    def calculate_overlap(self, bbox1, bbox2):
        """
        计算两个边界框的重叠率
        """
        x1, y1, w1, h1 = bbox1
        x2, y2, w2, h2 = bbox2
        
        # 计算交集
        x_left = max(x1, x2)
        y_top = max(y1, y2)
        x_right = min(x1 + w1, x2 + w2)
        y_bottom = min(y1 + h1, y2 + h2)
        
        if x_right < x_left or y_bottom < y_top:
            return 0.0
        
        intersection = (x_right - x_left) * (y_bottom - y_top)
        area1 = w1 * h1
        area2 = w2 * h2
        union = area1 + area2 - intersection
        
        return intersection / union if union > 0 else 0.0
    
    def is_bbox_inside(self, bbox1, bbox2, threshold=0.8):
        """
        检查bbox1是否在bbox2内部
        """
        x1, y1, w1, h1 = bbox1
        x2, y2, w2, h2 = bbox2
        
        # 计算bbox1在bbox2中的比例
        x_left = max(x1, x2)
        y_top = max(y1, y2)
        x_right = min(x1 + w1, x2 + w2)
        y_bottom = min(y1 + h1, y2 + h2)
        
        if x_right < x_left or y_bottom < y_top:
            return False
        
        intersection = (x_right - x_left) * (y_bottom - y_top)
        area1 = w1 * h1
        
        return (intersection / area1) > threshold
    
    def remove_duplicate_masks(self, masks, overlap_threshold=0.3):
        """
        移除重复的masks，保留面积更大、质量更高的
        """
        if not masks:
            return []
        
        # 按质量排序（结合面积和稳定性分数）
        masks_sorted = sorted(masks, key=lambda x: x['area'] * x['stability_score'], reverse=True)
        
        filtered_masks = []
        
        for current_mask in masks_sorted:
            current_bbox = current_mask['bbox']
            should_keep = True
            
            for kept_mask in filtered_masks:
                kept_bbox = kept_mask['bbox']
                
                # 检查重叠度
                overlap = self.calculate_overlap(current_bbox, kept_bbox)
                
                # 检查是否一个包含另一个
                current_inside_kept = self.is_bbox_inside(current_bbox, kept_bbox)
                kept_inside_current = self.is_bbox_inside(kept_bbox, current_bbox)
                
                if overlap > overlap_threshold or current_inside_kept:
                    should_keep = False
                    break
                elif kept_inside_current:
                    # 当前mask包含了已保留的mask，替换它
                    filtered_masks.remove(kept_mask)
                    break
            
            if should_keep:
                filtered_masks.append(current_mask)
        
        return filtered_masks
    
    def detect_info_regions(self, image, masks):
        """
        专门检测信息区域，并确保完整性
        """
        height, width = image.shape[:2]
        info_masks = []
        
        # 检测底部信息区域（通常包含技术要求、标题栏）
        bottom_region_y = int(height * 0.75)  # 底部25%区域
        
        # 检测右侧信息区域（有时标题栏在右侧）
        right_region_x = int(width * 0.75)   # 右侧25%区域
        
        # 首先识别底部大区域，避免过度分割
        bottom_masks = []
        right_masks = []
        other_masks = []
        
        for mask in masks:
            bbox = mask['bbox']
            x, y, w, h = bbox
            center_y = y + h/2
            center_x = x + w/2
            
            # 判断是否在底部信息区域
            is_bottom_info = center_y > bottom_region_y
            is_right_info = center_x > right_region_x
            
            # 面积判断（信息区域通常比主视图小，但不能太小）
            total_area = height * width
            area_ratio = mask['area'] / total_area
            is_reasonable_size = 0.002 < area_ratio < 0.3  # 放宽面积限制
            
            if is_bottom_info and is_reasonable_size:
                bottom_masks.append(mask)
            elif is_right_info and is_reasonable_size:
                right_masks.append(mask)
            else:
                other_masks.append(mask)
        
        # 合并底部的小片段为完整的信息区域
        if bottom_masks:
            merged_bottom = self.merge_bottom_info_regions(bottom_masks, width, height)
            if merged_bottom:
                merged_bottom['is_info_region'] = True
                merged_bottom['info_type'] = ['bottom_info', 'merged_region']
                info_masks.append(merged_bottom)
                print(f"合并了 {len(bottom_masks)} 个底部片段为完整信息区域")
        
        # 处理右侧信息区域
        for mask in right_masks:
            mask['is_info_region'] = True
            mask['info_type'] = ['right_info']
            info_masks.append(mask)
        
        # 保留其他masks用于主视图分析
        for mask in other_masks:
            if not mask.get('is_info_region', False):
                info_masks.append(mask)
        
        return info_masks
    
    def merge_bottom_info_regions(self, bottom_masks, image_width, image_height):
        """
        将底部的小片段合并为完整的信息区域
        """
        if not bottom_masks:
            return None
        
        # 计算底部信息区域的整体边界
        all_x = []
        all_y = []
        total_area = 0
        total_stability = 0
        
        for mask in bottom_masks:
            bbox = mask['bbox']
            x, y, w, h = bbox
            all_x.extend([x, x + w])
            all_y.extend([y, y + h])
            total_area += mask['area']
            total_stability += mask.get('stability_score', 0.8)
        
        # 创建包含所有底部片段的大区域
        min_x = max(0, min(all_x) - 20)  # 向左扩展20像素
        max_x = min(image_width, max(all_x) + 20)  # 向右扩展20像素
        min_y = max(0, min(all_y) - 10)  # 向上稍微扩展
        max_y = image_height  # 延伸到图像底部
        
        # 创建合并后的mask
        merged_mask = {
            'bbox': [min_x, min_y, max_x - min_x, max_y - min_y],
            'area': total_area,
            'stability_score': total_stability / len(bottom_masks),
            'predicted_iou': 0.9,  # 给予高置信度
            'is_merged': True,
            'original_count': len(bottom_masks)
        }
        
        return merged_mask
    
    def create_bottom_info_region(self, image_shape):
        """
        创建一个包含完整底部信息的保护区域
        """
        h, w = image_shape[:2]
        
        # 定义底部信息区域（底部20%）
        bottom_start_y = int(h * 0.8)
        
        # 创建底部完整信息区域
        bottom_info_mask = {
            'bbox': [0, bottom_start_y, w, h - bottom_start_y],
            'area': w * (h - bottom_start_y),
            'stability_score': 0.95,  # 高稳定性分数
            'predicted_iou': 0.95,
            'is_info_region': True,
            'info_type': ['bottom_info', 'protected_region'],
            'is_protected': True
        }
        
        return bottom_info_mask
    
    def expand_info_regions(self, image, info_masks):
        """
        扩展信息区域，特别是对合并的底部区域进行智能扩展
        """
        height, width = image.shape[:2]
        expanded_masks = []
        
        for mask in info_masks:
            # 跳过非信息区域
            if not mask.get('is_info_region', False):
                expanded_masks.append(mask)
                continue
                
            bbox = mask['bbox']
            x, y, w, h = bbox
            
            # 对合并的底部区域进行特殊处理
            if 'merged_region' in mask.get('info_type', []):
                # 合并区域已经包含了完整的底部信息，只需要少量扩展
                expand_x = max(10, int(w * 0.05))  # 5%或至少10像素的水平扩展
                expand_y = max(5, int(h * 0.02))   # 2%或至少5像素的垂直扩展
                
                new_x = max(0, x - expand_x)
                new_y = max(0, y - expand_y)
                new_w = min(width - new_x, w + 2 * expand_x)
                new_h = min(height - new_y, h + expand_y)  # 底部已经到边界，不需要向下扩展
                
                print(f"扩展合并底部区域: 原始({x}, {y}, {w}, {h}) -> 扩展后({new_x}, {new_y}, {new_w}, {new_h})")
                
            # 对其他信息区域的标准处理
            else:
                expand_ratio = 0.15  # 15%的标准扩展
                expand_x = int(w * expand_ratio)
                expand_y = int(h * expand_ratio)
                
                # 特别处理底部信息区域
                if 'bottom_info' in mask.get('info_type', []):
                    # 底部信息区域向左右扩展更多
                    expand_x = int(w * 0.25)
                    expand_y = int(h * 0.2)
                
                new_x = max(0, x - expand_x)
                new_y = max(0, y - expand_y)
                new_w = min(width - new_x, w + 2 * expand_x)
                new_h = min(height - new_y, h + 2 * expand_y)
            
            # 更新mask
            expanded_mask = mask.copy()
            expanded_mask['bbox'] = [new_x, new_y, new_w, new_h]
            expanded_mask['area'] = new_w * new_h
            expanded_masks.append(expanded_mask)
        
        return expanded_masks
    
    def post_process_text_regions(self, masks, image_shape):
        """对文本区域进行特殊的后处理，优化文本捕获"""
        processed_masks = []
        h, w = image_shape[:2]
        
        for mask in masks:
            bbox = mask['bbox']
            x, y, w_bbox, h_bbox = bbox
            
            # 检查是否在信息区域内
            in_bottom_info = y > h * 0.7
            in_right_info = x > w * 0.7
            
            # 如果在信息区域内，进行文本优化处理
            if in_bottom_info or in_right_info:
                # 计算长宽比来判断文本类型
                aspect_ratio = max(w_bbox, h_bbox) / min(w_bbox, h_bbox)
                
                # 文本行（水平文本）的特殊处理
                if aspect_ratio > 3 and w_bbox > h_bbox:
                    # 水平文本行 - 向左右扩展以包含完整文本
                    expand_h = int(w_bbox * 0.2)  # 20%的水平扩展
                    expand_v = int(h_bbox * 0.3)  # 30%的垂直扩展
                    
                    new_x = max(0, x - expand_h)
                    new_w = min(w, x + w_bbox + expand_h) - new_x
                    new_y = max(0, y - expand_v)
                    new_h = min(h, y + h_bbox + expand_v) - new_y
                    
                # 文本块（多行文本）的处理
                elif aspect_ratio <= 3:
                    # 均匀扩展
                    expand_ratio = 0.25
                    expand_h = int(w_bbox * expand_ratio)
                    expand_v = int(h_bbox * expand_ratio)
                    
                    new_x = max(0, x - expand_h)
                    new_w = min(w, x + w_bbox + expand_h) - new_x
                    new_y = max(0, y - expand_v)
                    new_h = min(h, y + h_bbox + expand_v) - new_y
                
                # 垂直文本的处理
                else:
                    # 垂直文本 - 向上下扩展
                    expand_v = int(h_bbox * 0.2)  # 20%的垂直扩展
                    expand_h = int(w_bbox * 0.3)  # 30%的水平扩展
                    
                    new_x = max(0, x - expand_h)
                    new_w = min(w, x + w_bbox + expand_h) - new_x
                    new_y = max(0, y - expand_v)
                    new_h = min(h, y + h_bbox + expand_v) - new_y
                
                # 更新mask信息
                mask['bbox'] = [new_x, new_y, new_w, new_h]
                mask['area'] = new_w * new_h
                
                # 标记为已优化的文本区域
                mask['text_optimized'] = True
            
            processed_masks.append(mask)
        
        return processed_masks
    
    def sort_masks_by_importance(self, masks):
        """
        按重要性对masks排序，优先保留完整的信息区域和高质量工程视图
        """
        def calculate_importance_score(mask):
            base_score = mask['area'] * mask['stability_score']
            
            # 保护区域最高优先级
            if mask.get('is_protected', False):
                base_score *= 3.0  # 保护区域优先级最高
            
            # 合并区域高优先级
            elif mask.get('is_merged', False):
                base_score *= 2.5  # 合并区域优先级很高
            
            # 信息区域加权
            elif mask.get('is_info_region', False):
                base_score *= 1.8  # 信息区域优先级提高80%
                
                # 底部信息区域通常更重要
                if 'bottom_info' in mask.get('info_type', []):
                    base_score *= 1.4
                    
                # 受保护的底部区域
                if mask.get('is_bottom_protected', False):
                    base_score *= 1.6
            
            return base_score
        
        return sorted(masks, key=calculate_importance_score, reverse=True)
    
    def expand_bbox_to_include_text(self, image, bbox, expansion_ratio=0.15):
        """
        扩展边界框以包含周围的文本和标注，增加扩展比例以获得更多上下文
        """
        x, y, w, h = bbox
        
        # 增加扩展量，确保包含更多周围信息
        expand_x = int(w * expansion_ratio)
        expand_y = int(h * expansion_ratio)
        
        # 扩展边界框
        new_x = max(0, int(x - expand_x))
        new_y = max(0, int(y - expand_y))
        new_w = min(image.shape[1] - new_x, int(w + 2 * expand_x))
        new_h = min(image.shape[0] - new_y, int(h + 2 * expand_y))
        
        return [new_x, new_y, new_w, new_h]
    
    def filter_masks_by_engineering_criteria(self, masks, image_shape):
        """
        根据工程图纸特征过滤masks，特别保护底部信息区域的完整性
        """
        h, w = image_shape[:2]
        total_area = h * w
        filtered_masks = []
        
        # 定义底部保护区域
        bottom_protect_y = int(h * 0.8)  # 底部20%区域
        
        for mask in masks:
            area = mask['area']
            bbox = mask['bbox']
            x, y, width, height = bbox
            
            # 检查是否在底部保护区域
            is_in_bottom_protect = y > bottom_protect_y
            
            # 检查是否在信息区域
            in_bottom_info = y > h * 0.7  # 底部30%区域
            in_right_info = x > w * 0.7   # 右侧30%区域
            is_info_region = in_bottom_info or in_right_info
            
            # 面积过滤 - 对底部保护区域使用更严格的标准防止过度分割
            area_ratio = area / total_area
            if is_in_bottom_protect:
                # 底部保护区域：只保留较大的区域，避免碎片化
                min_area_ratio = 0.008   # 0.8% - 相对较大的区域
                max_area_ratio = 0.5     # 50%
            elif is_info_region:
                # 其他信息区域允许较小的区域
                min_area_ratio = 0.002   # 0.2%
                max_area_ratio = 0.3     # 30%
            else:
                # 主绘图区域保持合理标准
                min_area_ratio = 0.003   # 0.3%
                max_area_ratio = 0.7     # 70%
            
            if not (min_area_ratio <= area_ratio <= max_area_ratio):
                continue
            
            # 长宽比过滤 - 对底部保护区域更严格
            aspect_ratio = max(width, height) / max(min(width, height), 1)
            if is_in_bottom_protect:
                # 底部保护区域不允许过于细长的条带
                max_aspect_ratio = 8  # 较严格的长宽比
            elif is_info_region:
                # 其他文本区域允许更极端的长宽比
                max_aspect_ratio = 25  
            else:
                # 主绘图区域保持合理长宽比
                max_aspect_ratio = 15
            
            if aspect_ratio > max_aspect_ratio:
                continue
            
            # 形状复杂度过滤
            perimeter = 2 * (width + height)
            if perimeter > 0:
                compactness = 4 * np.pi * area / (perimeter * perimeter)
                if is_in_bottom_protect:
                    min_compactness = 0.1  # 底部保护区域需要更高的紧凑度
                elif is_info_region:
                    min_compactness = 0.02
                else:
                    min_compactness = 0.05
                
                if compactness < min_compactness:
                    continue
            
            # 边界检查
            margin = 10
            if (x < margin and not is_info_region) or y < margin:
                if area < total_area * 0.05:
                    continue
            
            # 特殊标记保护区域
            if is_in_bottom_protect:
                mask['is_bottom_protected'] = True
                mask['priority_boost'] = 1.5
            elif is_info_region:
                mask['is_protected_text'] = True
                mask['priority_boost'] = 1.3
            
            filtered_masks.append(mask)
        
        return filtered_masks
    
    def merge_nearby_masks(self, masks, distance_threshold=100):
        """
        合并相近的小masks
        """
        if len(masks) < 2:
            return masks
        
        # 计算mask中心点
        centers = []
        for mask in masks:
            bbox = mask['bbox']
            center_x = bbox[0] + bbox[2] / 2
            center_y = bbox[1] + bbox[3] / 2
            centers.append([center_x, center_y])
        
        # 使用DBSCAN聚类
        clustering = DBSCAN(eps=distance_threshold, min_samples=1).fit(centers)
        
        merged_masks = []
        for cluster_id in set(clustering.labels_):
            cluster_masks = [masks[i] for i, label in enumerate(clustering.labels_) 
                           if label == cluster_id]
            
            if len(cluster_masks) == 1:
                merged_masks.append(cluster_masks[0])
            else:
                # 合并cluster中的masks
                merged_mask = self.merge_mask_cluster(cluster_masks)
                if merged_mask:
                    merged_masks.append(merged_mask)
        
        return merged_masks
    
    def merge_mask_cluster(self, cluster_masks):
        """
        合并一个cluster中的多个masks
        """
        if not cluster_masks:
            return None
        
        # 计算合并后的边界框
        min_x = min(mask['bbox'][0] for mask in cluster_masks)
        min_y = min(mask['bbox'][1] for mask in cluster_masks)
        max_x = max(mask['bbox'][0] + mask['bbox'][2] for mask in cluster_masks)
        max_y = max(mask['bbox'][1] + mask['bbox'][3] for mask in cluster_masks)
        
        merged_bbox = [int(min_x), int(min_y), int(max_x - min_x), int(max_y - min_y)]
        
        # 计算合并后的属性
        total_area = sum(mask['area'] for mask in cluster_masks)
        avg_stability = np.mean([mask['stability_score'] for mask in cluster_masks])
        avg_iou = np.mean([mask['predicted_iou'] for mask in cluster_masks])
        
        return {
            'bbox': merged_bbox,
            'area': total_area,
            'stability_score': avg_stability,
            'predicted_iou': avg_iou
        }
    
    def split_image(self, image_path, output_dir=None, visualize=True):
        """
        改进的图像分割主函数，优化性能和稳定性
        """
        # 读取图像
        original_image = cv2.imread(image_path)
        if original_image is None:
            raise ValueError(f"无法读取图像: {image_path}")
        
        print(f"正在处理图像: {image_path}")
        print(f"原始图像尺寸: {original_image.shape}")
        
        # 预处理图像以减少计算负载
        processed_image, scale_factor = self.preprocess_image(original_image)
        image_rgb = cv2.cvtColor(processed_image, cv2.COLOR_BGR2RGB)
        
        if scale_factor != 1.0:
            print(f"图像已缩放，缩放比例: {scale_factor:.3f}")
        
        # 使用轻量级生成器生成masks
        all_masks = []
        for i, generator in enumerate(self.mask_generators):
            try:
                print(f"正在使用生成器 {i+1} 生成masks...")
                
                # 清理内存
                if self.device == "cuda":
                    torch.cuda.empty_cache()
                
                # 使用threading实现超时保护（Windows兼容）
                import threading
                import queue
                
                def generate_masks(q):
                    try:
                        masks = generator.generate(image_rgb)
                        q.put(('success', masks))
                    except Exception as e:
                        q.put(('error', e))
                
                q = queue.Queue()
                thread = threading.Thread(target=generate_masks, args=(q,))
                thread.daemon = True
                thread.start()
                
                # 等待最多5分钟
                thread.join(timeout=300)
                
                if thread.is_alive():
                    print(f"生成器 {i+1} 超时，跳过...")
                    continue
                
                try:
                    result_type, result = q.get_nowait()
                    if result_type == 'success':
                        all_masks.extend(result)
                        print(f"生成器 {i+1} 生成了 {len(result)} 个masks")
                    else:
                        print(f"生成器 {i+1} 出错: {result}")
                        continue
                except queue.Empty:
                    print(f"生成器 {i+1} 没有返回结果")
                    continue
                    
            except Exception as e:
                print(f"生成器 {i+1} 出错: {e}")
                print("跳过这个生成器，继续处理...")
                # 清理内存
                if self.device == "cuda":
                    torch.cuda.empty_cache()
                continue
        
        if not all_masks:
            print("所有生成器都失败了，无法生成masks")
            return []
        
        print(f"总共生成了 {len(all_masks)} 个初始masks")
        
        # 将bbox缩放回原始图像尺寸
        if scale_factor != 1.0:
            for mask in all_masks:
                mask['bbox'] = self.scale_bbox(mask['bbox'], scale_factor)
                mask['area'] = int(mask['area'] / (scale_factor * scale_factor))
        
        # 根据工程图纸特征过滤（使用原始图像尺寸）
        filtered_masks = self.filter_masks_by_engineering_criteria(all_masks, original_image.shape)
        print(f"工程特征过滤后剩余 {len(filtered_masks)} 个masks")
        
        # 移除重复masks
        unique_masks = self.remove_duplicate_masks(filtered_masks)
        print(f"移除重复后剩余 {len(unique_masks)} 个masks")
        
        # 合并相近的小masks
        merged_masks = self.merge_nearby_masks(unique_masks)
        print(f"合并相近masks后剩余 {len(merged_masks)} 个masks")
        
        # 创建底部信息保护区域
        bottom_protection = self.create_bottom_info_region(original_image.shape)
        print("创建底部信息保护区域")
        
        # 专门检测和处理信息区域
        info_masks = self.detect_info_regions(original_image, merged_masks)
        
        # 将保护区域添加到信息masks中
        info_masks.append(bottom_protection)
        print(f"检测到 {len(info_masks)} 个信息区域（包含保护区域）")
        
        # 扩展信息区域
        expanded_info_masks = self.expand_info_regions(original_image, info_masks)
        
        # 将扩展后的信息区域与其他masks合并
        non_info_masks = [mask for mask in merged_masks if not mask.get('is_info_region', False)]
        all_final_masks = non_info_masks + expanded_info_masks
        
        # 对文本区域进行特殊后处理
        text_optimized_masks = self.post_process_text_regions(all_final_masks, original_image.shape)
        print(f"文本区域优化完成")
        
        # 再次去重（因为扩展和后处理可能造成重叠）
        final_masks_deduped = self.remove_duplicate_masks(text_optimized_masks, overlap_threshold=0.4)
        print(f"信息区域处理后剩余 {len(final_masks_deduped)} 个masks")
        
        if not final_masks_deduped:
            print("未找到有效的视图区域")
            return []
        
        # 按质量和重要性排序
        final_masks = self.sort_masks_by_importance(final_masks_deduped)[:20]  # 增加到20个视图
        
        # 设置输出目录
        if output_dir is None:
            output_dir = os.path.join(
                os.path.dirname(image_path), 
                f"{Path(image_path).stem}_improved_views"
            )
        
        Path(output_dir).mkdir(parents=True, exist_ok=True)
        
        # 保存分割结果
        saved_files = []
        view_info = []
        
        for i, mask_data in enumerate(final_masks, 1):
            bbox = mask_data['bbox']
            
            # 扩展边界框以包含周围文本
            expanded_bbox = self.expand_bbox_to_include_text(original_image, bbox)
            x, y, w, h = expanded_bbox
            
            # 确保所有坐标都是整数
            x, y, w, h = int(x), int(y), int(w), int(h)
            
            print(f"处理视图 {i}: 扩展后bbox=({x}, {y}, {w}, {h})")
            
            # 确保坐标有效
            x = max(0, min(x, original_image.shape[1] - 1))
            y = max(0, min(y, original_image.shape[0] - 1))
            w = min(w, original_image.shape[1] - x)
            h = min(h, original_image.shape[0] - y)
            
            if w <= 0 or h <= 0:
                print(f"跳过无效的边界框: {i}")
                continue
            
            # 提取视图，确保高质量
            view = original_image[y:y+h, x:x+w]
            
            if view.size == 0:
                print(f"跳过空视图: {i}")
                continue
            
            # 如果视图太小，进行高质量放大
            min_dimension = 600  # 最小尺寸
            if min(view.shape[:2]) < min_dimension:
                scale_factor = min_dimension / min(view.shape[:2])
                new_width = int(view.shape[1] * scale_factor)
                new_height = int(view.shape[0] * scale_factor)
                view = cv2.resize(view, (new_width, new_height), interpolation=cv2.INTER_CUBIC)
                print(f"视图 {i} 放大到: {new_width}x{new_height}")
            
            # 保存高质量视图
            output_filename = f"improved_view_{i:02d}.png"
            output_path = os.path.join(output_dir, output_filename)
            
            # 使用高质量参数保存PNG
            cv2.imwrite(output_path, view, [cv2.IMWRITE_PNG_COMPRESSION, 0])  # 无压缩
            
            saved_files.append(output_path)
            view_info.append({
                'view_id': i,
                'filename': output_filename,
                'bbox': [x, y, w, h],
                'area': int(mask_data['area']),
                'stability_score': float(mask_data['stability_score']),
                'predicted_iou': float(mask_data['predicted_iou']),
                'quality_score': float(mask_data['area'] * mask_data['stability_score'])
            })
            
            print(f"保存视图 {i}: {output_filename}")
        
        # 保存视图信息
        info_file = os.path.join(output_dir, "improved_views_info.json")
        with open(info_file, 'w', encoding='utf-8') as f:
            json.dump({
                'source_image': image_path,
                'total_views': len(view_info),
                'views': view_info
            }, f, indent=2, ensure_ascii=False)
        
        # 可视化结果
        if visualize:
            self.visualize_results(original_image, final_masks, output_dir)
        
        print(f"\n分割完成！共生成 {len(saved_files)} 个改进视图")
        print(f"输出目录: {output_dir}")
        
        return saved_files
    
    def visualize_results(self, image, masks, output_dir):
        """
        可视化分割结果
        """
        vis_image = image.copy()
        
        colors = [
            (0, 255, 0), (255, 0, 0), (0, 0, 255), 
            (255, 255, 0), (255, 0, 255), (0, 255, 255),
            (128, 255, 0), (255, 128, 0), (128, 0, 255),
            (255, 128, 128), (128, 255, 128), (128, 128, 255)
        ]
        
        for i, mask_data in enumerate(masks):
            color = colors[i % len(colors)]
            bbox = mask_data['bbox']
            
            # 扩展边界框用于显示
            expanded_bbox = self.expand_bbox_to_include_text(image, bbox)
            x, y, w, h = expanded_bbox
            
            # 确保所有坐标都是整数
            x, y, w, h = int(x), int(y), int(w), int(h)
            
            # 绘制边界框
            cv2.rectangle(vis_image, (x, y), (x + w, y + h), color, 4)
            cv2.putText(vis_image, f'View {i+1}', (x, y-15), 
                       cv2.FONT_HERSHEY_SIMPLEX, 1.2, color, 3)
            
            # 绘制质量分数
            quality = mask_data['area'] * mask_data['stability_score']
            cv2.putText(vis_image, f'Q: {quality:.0f}', (x, y+h+30), 
                       cv2.FONT_HERSHEY_SIMPLEX, 0.8, color, 2)
        
        # 保存可视化结果
        vis_path = os.path.join(output_dir, "improved_visualization.png")
        cv2.imwrite(vis_path, vis_image)
        print(f"可视化结果保存到: {vis_path}")


def main():
    parser = argparse.ArgumentParser(description="改进的SAM工程图纸分割")
    
    parser.add_argument('image_path', help='输入图像路径')
    parser.add_argument('--output', '-o', dest='output_dir', help='输出目录')
    parser.add_argument('--model', default='vit_b', choices=['vit_b', 'vit_l', 'vit_h'],
                       help='SAM模型类型')
    parser.add_argument('--checkpoint', help='模型检查点路径')
    parser.add_argument('--no-visualize', action='store_true', help='不生成可视化结果')
    
    args = parser.parse_args()
    
    try:
        splitter = ImprovedSAMDrawingSplitter(
            model_type=args.model,
            checkpoint_path=args.checkpoint
        )
        
        output_files = splitter.split_image(
            image_path=args.image_path,
            output_dir=args.output_dir,
            visualize=not args.no_visualize
        )
        
        print(f"\n✅ 改进分割完成！")
        print("生成的视图文件:")
        for file_path in output_files:
            print(f"  - {file_path}")
            
    except Exception as e:
        print(f"❌ 分割失败: {e}")
        import traceback
        traceback.print_exc()


if __name__ == "__main__":
    main()